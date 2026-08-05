mod app;
mod blocking;
mod config;
mod db;
mod dlp;
mod error;
use crate::error::not_found;
mod handlers;
mod middleware;
mod providers;
mod rate_limiter;
mod ssrf;
mod utils;

pub(crate) use blocking::run_blocking;

#[cfg(test)]
mod test_utils;

use axum::http::{HeaderValue, Method, header};
use axum::routing::{Router, delete, get, post, put};
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::info;

use handlers::analytics::{model_dist, requests, token_dist, tokens};
use handlers::apikeys::{create_api_key, delete_api_key, list_api_keys, update_api_key};
use handlers::auth::{login, logout, session_check};
use handlers::logs::list_proxy_logs;
use handlers::models::{create_model, delete_model, get_model, list_models, update_model};
use handlers::providers::{
    create_provider, delete_provider, get_provider, get_provider_api_key, list_provider_types,
    list_providers, update_provider,
};
use handlers::proxy::{
    chat_completions, completions, embeddings, health, list_models_proxy, responses,
};
use handlers::stats::overview_stats;
use handlers::users::change_password;
use middleware::{
    access_log_middleware, admin_auth_middleware, auth_middleware, login_rate_limit_middleware,
};

#[tokio::main]
async fn main() {
    let config_path = parse_config_path();
    let config = match config::ConfigApp::new(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };

    init_logging(&config.log);

    let state = match app::AppState::new(config.clone()) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Init failed: {}", e);
            std::process::exit(1);
        }
    };
    let log_manager = state.log_manager.clone();
    let shutdown_token = state.shutdown_token.clone();
    let graceful_timeout = Duration::from_secs(config.server.graceful_timeout_secs);

    let app = build_app(state);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("Invalid host:port");

    info!("ait starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        });

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown({
        let token = shutdown_token.clone();
        async move {
            token.cancelled().await;
        }
    })
    .into_future();

    tokio::pin!(server);

    let shutdown_watcher = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
        tracing::info!("Shutdown requested, waiting for in-flight requests...");
        shutdown_token.cancel();

        tokio::select! {
            _ = tokio::time::sleep(graceful_timeout) => {
                tracing::warn!("Graceful shutdown timeout, forcing exit");
                log_manager.shutdown();
                std::process::exit(0);
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::warn!("Forced shutdown via Ctrl+C");
                log_manager.shutdown();
                std::process::exit(0);
            }
        }
    };

    tokio::select! {
        result = &mut server => {
            result.unwrap_or_else(|e| {
                tracing::error!("Server error: {}", e);
                std::process::exit(1);
            });
        }
        _ = shutdown_watcher => {}
    }

    log_manager.shutdown();
}

fn parse_config_path() -> Option<String> {
    parse_config_path_from(std::env::args())
}

/// Testable core of `parse_config_path`: walk the argument list and return the
/// value following `-c`/`--config`, if present.
fn parse_config_path_from<I: Iterator<Item = String>>(mut args: I) -> Option<String> {
    args.next(); // skip program name
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                return args.next().or_else(|| {
                    tracing::error!("-c/--config requires a path argument");
                    std::process::exit(1);
                });
            }
            _ => {}
        }
    }
    None
}

fn parse_level(s: &str) -> tracing::Level {
    match s.to_lowercase().as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO,
    }
}

fn init_logging(cfg: &config::LogConfig) {
    let filter = format!("info,ait={},axum={}", cfg.level, cfg.axum);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .init();
}

fn cors_layer(allowed_origins: &[String], allow_credentials: bool) -> CorsLayer {
    let methods = [Method::GET, Method::POST, Method::PUT, Method::DELETE];
    let headers = [
        header::AUTHORIZATION,
        header::CONTENT_TYPE,
        header::ACCEPT,
        header::COOKIE,
        header::HeaderName::from_static("x-requested-with"),
    ];

    if allowed_origins.is_empty() {
        return CorsLayer::new()
            .allow_origin(AllowOrigin::list(Vec::<HeaderValue>::new()))
            .allow_methods(methods)
            .allow_headers(headers);
    }

    if allowed_origins.iter().any(|o| o == "*") {
        return if allow_credentials {
            // `*` combined with credentials is rejected by browsers; mirror the
            // request origin instead so cross-origin cookie auth can work
            CorsLayer::new()
                .allow_origin(AllowOrigin::mirror_request())
                .allow_methods(methods)
                .allow_headers(headers)
                .allow_credentials(true)
        } else {
            CorsLayer::permissive()
                .allow_methods(methods)
                .allow_headers(headers)
        };
    }

    let origins: Vec<_> = allowed_origins
        .iter()
        .map(|o| {
            o.parse()
                .expect("Invalid origin in security.cors_allowed_origins")
        })
        .collect();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(methods)
        .allow_headers(headers)
        .allow_credentials(allow_credentials)
}

fn build_app(state: app::AppState) -> Router {
    // Auth routes — nested under /auth so unmatched paths return 404
    let login_route =
        Router::new()
            .route("/login", post(login))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                login_rate_limit_middleware,
            ));
    let auth_routes = Router::new()
        .merge(login_route)
        .route("/logout", post(logout))
        .route("/session", get(session_check))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ))
        .fallback(|| async { not_found("404 not found") });

    // Admin API routes (admin auth required)
    let admin_api = Router::new()
        // Provider management
        .route("/providers", post(create_provider))
        .route("/providers", get(list_providers))
        .route("/providers/{id}", get(get_provider))
        .route("/providers/{id}", put(update_provider))
        .route("/providers/{id}", delete(delete_provider))
        .route("/providers/{id}/api-key", get(get_provider_api_key))
        .route("/provider-types", get(list_provider_types))
        // Model management
        .route("/models", post(create_model))
        .route("/models", get(list_models))
        .route("/models/{name}", get(get_model))
        .route("/models/{name}", put(update_model))
        .route("/models/{name}", delete(delete_model))
        // User management
        .route("/users/{username}/password", put(change_password))
        // API key management
        .route("/users/{username}/api-keys", get(list_api_keys))
        .route("/users/{username}/api-keys", post(create_api_key))
        .route("/users/{username}/api-keys/{key}", put(update_api_key))
        .route("/users/{username}/api-keys/{key}", delete(delete_api_key))
        // Overview statistics
        .route("/stats", get(overview_stats))
        // Analytics
        .route("/data/requests", get(requests))
        .route("/data/tokens", get(tokens))
        .route("/data/model-dist", get(model_dist))
        .route("/data/token-dist", get(token_dist))
        // Proxy logs
        .route("/data/proxy-log", get(list_proxy_logs))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ))
        .fallback(|| async { not_found("404 not found") });

    // Health check (no auth required)
    let health_route = Router::new().route("/health", get(health));

    // OpenAI-compatible proxy routes — nested under /v1 so unmatched paths return 404
    let proxy_api_v1 = Router::new()
        .route("/chat/completions", post(chat_completions))
        .route("/completions", post(completions))
        .route("/embeddings", post(embeddings))
        .route("/responses", post(responses))
        .route("/models", get(list_models_proxy))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .fallback(|| async { not_found("404 not found") });

    // Serve frontend static files
    let frontend_root = ServeDir::new("frontend/dist");
    let frontend_spa = frontend_root
        .clone()
        .fallback(ServeFile::new("frontend/dist/index.html"));

    let trace_level = parse_level(&state.config.log.tower_http_trace);
    let cors = cors_layer(
        &state.config.security.cors_allowed_origins,
        state.config.security.cors_allow_credentials,
    );

    Router::new()
        .nest_service("/static", frontend_root)
        .fallback_service(frontend_spa)
        .nest("/auth", auth_routes)
        .nest("/v1", proxy_api_v1)
        .merge(health_route)
        .nest("/api", Router::new().merge(admin_api))
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http().on_response(DefaultOnResponse::new().level(trace_level)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_config_path_no_flag_returns_none() {
        assert_eq!(parse_config_path_from(args(&["ait"]).into_iter()), None);
        assert_eq!(
            parse_config_path_from(args(&["ait", "-x", "foo"]).into_iter()),
            None
        );
    }

    #[test]
    fn parse_config_path_short_flag_returns_value() {
        assert_eq!(
            parse_config_path_from(args(&["ait", "-c", "config/test"]).into_iter()),
            Some("config/test".to_string())
        );
    }

    #[test]
    fn parse_config_path_long_flag_returns_value() {
        assert_eq!(
            parse_config_path_from(args(&["ait", "--config", "config/prod"]).into_iter()),
            Some("config/prod".to_string())
        );
    }

    #[test]
    fn parse_level_maps_all_levels() {
        assert_eq!(parse_level("trace"), tracing::Level::TRACE);
        assert_eq!(parse_level("debug"), tracing::Level::DEBUG);
        assert_eq!(parse_level("info"), tracing::Level::INFO);
        assert_eq!(parse_level("warn"), tracing::Level::WARN);
        assert_eq!(parse_level("error"), tracing::Level::ERROR);
        assert_eq!(parse_level("INFO"), tracing::Level::INFO);
        // Unknown values fall back to INFO.
        assert_eq!(parse_level("verbose"), tracing::Level::INFO);
    }

    fn cors_router(cors: CorsLayer) -> Router {
        Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(cors)
    }

    async fn get_with_origin(router: &Router, origin: &str) -> Option<String> {
        let request = Request::builder()
            .method(Method::GET)
            .uri("/")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    #[tokio::test]
    async fn cors_layer_empty_origins_sends_no_header() {
        let router = cors_router(cors_layer(&[], false));
        assert!(
            get_with_origin(&router, "https://evil.example.com")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn cors_layer_wildcard_without_credentials_is_permissive() {
        let router = cors_router(cors_layer(&["*".to_string()], false));
        assert_eq!(
            get_with_origin(&router, "https://evil.example.com")
                .await
                .as_deref(),
            Some("*")
        );
    }

    #[tokio::test]
    async fn cors_layer_wildcard_with_credentials_mirrors_origin() {
        let router = cors_router(cors_layer(&["*".to_string()], true));
        assert_eq!(
            get_with_origin(&router, "https://app.example.com")
                .await
                .as_deref(),
            Some("https://app.example.com")
        );
    }

    #[tokio::test]
    async fn cors_layer_specific_origins_allow_only_listed() {
        let router = cors_router(cors_layer(&["https://app.example.com".to_string()], false));
        assert_eq!(
            get_with_origin(&router, "https://app.example.com")
                .await
                .as_deref(),
            Some("https://app.example.com")
        );
        assert!(
            get_with_origin(&router, "https://other.example.com")
                .await
                .is_none()
        );
    }
}
