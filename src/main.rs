mod app;
mod config;
mod db;
mod error;
mod handlers;
mod middleware;
mod providers;

use axum::{
    routing::{delete, get, post, put, Router},
    Extension,
};
use tower_http::services::ServeDir;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing::info;

use handlers::admin::{
    create_model, create_provider, delete_model, delete_provider,
    get_provider, get_provider_api_key, list_models, list_providers,
    update_model, update_provider,
};
use handlers::apikeys::{
    create_api_key_handler, delete_api_key_handler, list_api_keys_handler,
};
use handlers::auth::{login_handler, logout_handler, register_handler, session_check};
use handlers::proxy::{chat_completions, completions, embeddings, health, list_models_proxy};
use handlers::users::{
    change_password_handler, delete_user_handler, list_users_handler, update_user_handler,
};
use middleware::{admin_auth_middleware, auth_middleware};

#[tokio::main]
async fn main() {
    init_logging();

    let config_path = parse_config_path();
    let config = match config::ConfigApp::new(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    let state = app::AppState::new(config.clone());

    let app = build_app(state, &config);

    let addr: SocketAddr =
        format!("{}:{}", config.server.host, config.server.port)
            .parse()
            .expect("Invalid host:port");

    info!("ait starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn parse_config_path() -> Option<String> {
    let mut args = std::env::args();
    args.next(); // skip program name
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                return args.next().or_else(|| {
                    eprintln!("error: -c/--config requires a path argument");
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

fn build_app(state: app::AppState, config: &config::ConfigApp) -> Router {
    // Auth routes (no admin middleware — they handle their own auth logic)
    let auth_route = Router::new()
        .route("/admin/login", post(login_handler))
        .route("/admin/register", post(register_handler))
        .route("/admin/logout", post(logout_handler))
        .route("/admin/session", get(session_check));

    // Admin API routes (admin auth required)
    let admin_api = Router::new()
        // Provider management
        .route("/admin/providers", post(create_provider))
        .route("/admin/providers", get(list_providers))
        .route("/admin/providers/{id}", get(get_provider))
        .route("/admin/providers/{id}", put(update_provider))
        .route("/admin/providers/{id}", delete(delete_provider))
        .route("/admin/providers/{id}/api-key", get(get_provider_api_key))
        // Model management
        .route("/admin/models", post(create_model))
        .route("/admin/models", get(list_models))
        .route("/admin/models/{name}", put(update_model))
        .route("/admin/models/{name}", delete(delete_model))
        // User management
        .route("/admin/users", get(list_users_handler))
        .route("/admin/users/{username}", put(update_user_handler))
        .route("/admin/users/{username}", delete(delete_user_handler))
        .route("/admin/users/{username}/password", put(change_password_handler))
        // API key management
        .route("/admin/users/{username}/api-keys", get(list_api_keys_handler))
        .route("/admin/users/{username}/api-keys", post(create_api_key_handler))
        .route("/admin/users/{username}/api-keys/{key}", delete(delete_api_key_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ));

    // Health check (no auth required)
    let health_route = Router::new()
        .route("/v1/health", get(health));

    // OpenAI-compatible proxy routes (auth required)
    let proxy_api = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/embeddings", post(embeddings))
        .route("/v1/models", get(list_models_proxy))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    // Serve frontend static files
    let frontend_service = ServeDir::new("frontend/dist");

    Router::new()
        .nest_service("/static", frontend_service.clone())
        .fallback_service(frontend_service)
        .merge(auth_route)
        .merge(admin_api)
        .merge(health_route)
        .merge(proxy_api)
        .with_state(state)
        .layer(Extension(config.clone()))
        .layer(TraceLayer::new_for_http())
}
