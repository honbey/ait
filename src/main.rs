mod app;
mod blocking;
mod config;
mod db;
mod error;
mod handlers;
mod middleware;
mod providers;
mod rate_limiter;

pub(crate) use blocking::run_blocking;

#[cfg(test)]
mod test_utils;

use axum::routing::{Router, delete, get, post, put};
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::info;

use handlers::admin::{
    create_model, create_provider, delete_model, delete_provider, get_provider,
    get_provider_api_key, list_models, list_provider_types, list_providers, update_model,
    update_provider,
};
use handlers::analytics::{requests, tokens};
use handlers::apikeys::{create_api_key, delete_api_key, list_api_keys, toggle_api_key};
use handlers::auth::{login, logout, register, session_check};
use handlers::proxy::{chat_completions, completions, embeddings, health, list_models_proxy};
use handlers::stats::dashboard_stats;
use handlers::users::{change_password, delete_user, list_users};
use middleware::{
    access_log_middleware, admin_auth_middleware, auth_middleware, login_rate_limit_middleware,
    register_rate_limit_middleware,
};

#[tokio::main]
async fn main() {
    init_logging();

    let config_path = parse_config_path();
    let config = match config::ConfigApp::new(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

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
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::warn!("Forced shutdown via Ctrl+C");
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
    let mut args = std::env::args();
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

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ait=debug,axum=info".into()),
        )
        .init();
}

fn build_app(state: app::AppState) -> Router {
    // Auth routes (no admin middleware — they handle their own auth logic)
    let login_route = Router::new().route("/auth/login", post(login)).layer(
        axum::middleware::from_fn_with_state(state.clone(), login_rate_limit_middleware),
    );
    let register_route = Router::new().route("/auth/register", post(register)).layer(
        axum::middleware::from_fn_with_state(state.clone(), register_rate_limit_middleware),
    );
    let auth_route = Router::new()
        .merge(login_route)
        .merge(register_route)
        .route("/auth/logout", post(logout))
        .route("/auth/session", get(session_check))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ));

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
        .route("/models/{name}", put(update_model))
        .route("/models/{name}", delete(delete_model))
        // User management
        .route("/users", get(list_users))
        .route("/users/{username}", delete(delete_user))
        .route("/users/{username}/password", put(change_password))
        // API key management
        .route("/users/{username}/api-keys", get(list_api_keys))
        .route("/users/{username}/api-keys", post(create_api_key))
        .route("/users/{username}/api-keys/{key}", put(toggle_api_key))
        .route("/users/{username}/api-keys/{key}", delete(delete_api_key))
        // Dashboard statistics
        .route("/stats", get(dashboard_stats))
        // Hourly-bucketed analytics
        .route("/data/requests", get(requests))
        .route("/data/tokens", get(tokens))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    // Health check (no auth required)
    let health_route = Router::new().route("/health", get(health));

    // OpenAI-compatible proxy routes (auth required)
    let proxy_api = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/models", get(list_models_proxy))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Serve frontend static files
    let frontend_root = ServeDir::new("frontend/dist");
    let frontend_spa = frontend_root
        .clone()
        .fallback(ServeFile::new("frontend/dist/index.html"));

    Router::new()
        .nest_service("/static", frontend_root)
        .fallback_service(frontend_spa)
        .merge(auth_route)
        .merge(proxy_api)
        .merge(health_route)
        .nest("/api", Router::new().merge(admin_api))
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .on_response(DefaultOnResponse::new().level(tracing::Level::DEBUG)),
        )
}
