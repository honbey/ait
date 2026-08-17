use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

use crate::app::AppState;

/// Header name set by the upstream authenticator (e.g. Authelia via nginx).
const REMOTE_USER_HEADER: &str = "remote-user";

#[derive(serde::Serialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub username: Option<String>,
}

/// Returns the identity of the caller as reported by the upstream
/// authenticator. Ait itself does not perform web-admin authentication;
/// `/api/*` is protected by a reverse proxy (e.g. nginx + Authelia) which
/// forwards the user identity via the `Remote-User` header.
pub async fn session_check(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let username = headers
        .get(REMOTE_USER_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let authenticated = username.is_some();
    (
        StatusCode::OK,
        Json(SessionResponse {
            authenticated,
            username,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_test_state, test_router};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Method, Request, header};
    use std::net::SocketAddr;
    use tower::ServiceExt;

    async fn session_with_header(header_value: Option<&str>) -> serde_json::Value {
        let (state, _dir) = create_test_state();
        let router = test_router(state);
        let mut builder = Request::builder().method(Method::GET).uri("/auth/session");
        if let Some(v) = header_value {
            builder = builder.header(REMOTE_USER_HEADER, v);
        }
        let mut request = builder.body(Body::empty()).unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 0))));
        let response = router.clone().oneshot(request).await.unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    #[tokio::test]
    async fn session_check_returns_remote_user_header() {
        let json = session_with_header(Some("alice")).await;
        assert_eq!(json["authenticated"], serde_json::Value::Bool(true));
        assert_eq!(json["username"], "alice");
    }

    #[tokio::test]
    async fn session_check_without_header_unauthenticated() {
        let json = session_with_header(None).await;
        assert_eq!(json["authenticated"], serde_json::Value::Bool(false));
        assert_eq!(json["username"], serde_json::Value::Null);
    }
}
