use std::net::{IpAddr, SocketAddr};

use axum::{
    Json,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;

use crate::error::{AitError, internal_error};
use crate::handlers::analytics::validate_ts_range;

/// Header name set by the upstream authenticator (e.g. Authelia via nginx).
const REMOTE_USER_HEADER: &str = "remote-user";

#[derive(Deserialize)]
pub struct StatsQuery {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

#[derive(Serialize)]
pub struct OverviewStats {
    pub provider_count: usize,
    pub model_count: usize,
    pub api_request_count: u64,
    pub token_consumption: u64,
    pub rpm: f64,
    pub tpm: f64,
    /// Caller identity reported by the upstream authenticator; used only for
    /// the overview greeting, not for authentication.
    pub username: Option<String>,
}

fn is_trusted_proxy(ip: IpAddr, trusted: &[IpAddr]) -> bool {
    trusted.contains(&ip)
}

/// Only trust the identity header when the direct peer is a known reverse
/// proxy; a client that reaches Ait directly can forge `Remote-User`.
fn remote_user_from_headers(
    headers: &HeaderMap,
    direct_ip: IpAddr,
    trusted_proxies: &[IpAddr],
) -> Option<String> {
    if !is_trusted_proxy(direct_ip, trusted_proxies) {
        return None;
    }
    headers
        .get(REMOTE_USER_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn overview_stats(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(q): Query<StatsQuery>,
) -> Result<Json<OverviewStats>, (StatusCode, Json<AitError>)> {
    let username =
        remote_user_from_headers(&headers, addr.ip(), &state.config.server.trusted_proxies);
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let range_mins = (range.end - range.start) as f64 / 60.0;

    let db = state.db.clone();
    let (provider_count, model_count) =
        crate::run_blocking(move || -> Result<(usize, usize), crate::db::DbError> {
            let p = db.count_providers()?;
            let m = db.count_models()?;
            Ok((p, m))
        })
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;
    let api_request_count = state
        .log_manager
        .total_requests(range.start, range.end)
        .await;
    let token_consumption = state.log_manager.total_tokens(range.start, range.end).await;

    let rpm = ((api_request_count as f64 / range_mins) * 100.0).round() / 100.0;
    let tpm = ((token_consumption as f64 / range_mins) * 100.0).round() / 100.0;

    Ok(Json(OverviewStats {
        provider_count,
        model_count,
        api_request_count,
        token_consumption,
        rpm,
        tpm,
        username,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        create_test_state_fast_logs, make_proxy_event, send_request, test_router,
    };
    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Method;
    use axum::http::Request;
    use chrono::Utc;
    use std::net::SocketAddr;
    use tower::ServiceExt;

    async fn setup() -> Router {
        let (state, _dir) = create_test_state_fast_logs();
        let router = test_router(state);
        router
    }

    async fn stats_with_remote_user(header_value: Option<&str>) -> serde_json::Value {
        let router = setup().await;
        let mut builder = Request::builder().method(Method::GET).uri("/api/stats");
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
    async fn overview_stats_returns_remote_user_header() {
        let json = stats_with_remote_user(Some("alice")).await;
        assert_eq!(json["username"], "alice");
    }

    #[tokio::test]
    async fn overview_stats_without_remote_user_header_returns_null() {
        let json = stats_with_remote_user(None).await;
        assert_eq!(json["username"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn overview_stats_ignores_remote_user_from_untrusted_peer() {
        let router = setup().await;
        let mut builder = Request::builder().method(Method::GET).uri("/api/stats");
        builder = builder.header(REMOTE_USER_HEADER, "attacker");
        let mut request = builder.body(Body::empty()).unwrap();
        // 10.0.0.1 is NOT in trusted_proxies (test_config uses 127.0.0.1, ::1)
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 0))));
        let response = router.oneshot(request).await.unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        assert_eq!(json["username"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn overview_stats_zero_without_data() {
        let router = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/stats?start_ts=0&end_ts=1",
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["provider_count"], 0);
        assert_eq!(resp.json["model_count"], 0);
        assert_eq!(resp.json["api_request_count"], 0);
        assert_eq!(resp.json["token_consumption"], 0);
        assert_eq!(resp.json["rpm"], 0.0);
    }

    #[tokio::test]
    async fn overview_stats_counts_written_proxy_events() {
        let (state, _dir) = create_test_state_fast_logs();
        let now = Utc::now().timestamp();
        state
            .log_manager
            .log_proxy(make_proxy_event("gpt-4", "success", 300));
        state
            .log_manager
            .log_proxy(make_proxy_event("llama", "success", 150));
        // Wait until the worker has flushed both events.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let count = state
                .log_manager
                .total_requests(now - 3600, now + 3600)
                .await;
            if count >= 2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for proxy events"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let range = crate::handlers::analytics::validate_ts_range(
            None,
            None,
            state.config.log.retention_days,
        )
        .unwrap();
        let _ = range;
        let router = test_router(state);

        // Explicit range with headroom: the default end (now, second precision)
        // can equal the event timestamp and the exclusive upper bound would
        // exclude the row.
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/stats?start_ts={}&end_ts={}", now - 3600, now + 3600),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["api_request_count"], 2);
        assert_eq!(resp.json["token_consumption"], 450);
        assert_eq!(resp.json["provider_count"], 0);
    }

    #[tokio::test]
    async fn overview_stats_rejects_invalid_timestamps() {
        let (router) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/stats?start_ts=99999999999999999999",
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }
}
