use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::analytics::AnalyticsError;
use crate::db::models::{PaginatedResponse, ProxyLogEntryResponse, ProxyLogQueryParams};
use crate::error::AitError;

#[derive(Deserialize)]
pub struct ProxyLogQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub model_name: Option<String>,
    pub provider_name: Option<String>,
    pub status: Option<String>,
    pub api_key_name: Option<String>,
    pub endpoint: Option<String>,
    pub is_streaming: Option<bool>,
    pub upstream_model: Option<String>,
    pub provider_type: Option<String>,
    pub client_ip: Option<String>,
}

const MAX_PER_PAGE: u64 = 100;
/// Upper bound for `page`. Without it `page * per_page` overflows and panics
/// the analytics worker thread, permanently disabling every analytics query.
const MAX_PAGE: u64 = 1_000_000;

pub async fn list_proxy_logs(
    State(state): State<AppState>,
    Query(q): Query<ProxyLogQuery>,
) -> Result<Json<PaginatedResponse<ProxyLogEntryResponse>>, (StatusCode, Json<AitError>)> {
    let page = q.page.unwrap_or(1).clamp(1, MAX_PAGE);
    let per_page = q.per_page.unwrap_or(20).clamp(1, MAX_PER_PAGE);

    if let Some(ts) = q.start_ts
        && chrono::DateTime::from_timestamp(ts, 0).is_none()
    {
        return Err(AitError::bad_request("invalid start_ts").into_response());
    }
    if let Some(ts) = q.end_ts
        && chrono::DateTime::from_timestamp(ts, 0).is_none()
    {
        return Err(AitError::bad_request("invalid end_ts").into_response());
    }

    let params = ProxyLogQueryParams {
        page,
        per_page,
        start_ts: q.start_ts,
        end_ts: q.end_ts,
        model_name: q.model_name,
        provider_name: q.provider_name,
        status: q.status,
        api_key_name: q.api_key_name,
        endpoint: q.endpoint,
        is_streaming: q.is_streaming,
        upstream_model: q.upstream_model,
        provider_type: q.provider_type,
        client_ip: q.client_ip,
    };

    let result = state
        .log_manager
        .query_proxy_logs(params)
        .await
        .map_err(AnalyticsError::into_response)?;

    Ok(Json(PaginatedResponse {
        items: result.items,
        total: result.total,
        page,
        per_page,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        create_test_state_fast_logs, make_proxy_event, send_request, test_router,
    };
    use axum::Router;
    use axum::http::Method;
    use chrono::Utc;

    async fn setup_with_events() -> (Router, i64) {
        let (state, _dir) = create_test_state_fast_logs();
        let now = Utc::now().timestamp();
        state
            .log_manager
            .log_proxy(make_proxy_event("gpt-4", "success", 100));
        state
            .log_manager
            .log_proxy(make_proxy_event("gpt-4", "error", 50));
        state
            .log_manager
            .log_proxy(make_proxy_event("llama", "success", 10));
        // Wait for the worker to flush all three events.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if state
                .log_manager
                .overview(now - 3600, now + 3600)
                .await
                .unwrap()
                .total_requests
                >= 3
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for proxy events"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let router = test_router(state);
        (router, now)
    }

    #[tokio::test]
    async fn list_proxy_logs_returns_all_rows_desc() {
        let (router, now) = setup_with_events().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!(
                "/api/data/proxy-log?start_ts={}&end_ts={}",
                now - 3600,
                now + 3600
            ),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["total"], 3);
        let items = resp.json["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        // Ordered by timestamp DESC: the last written event is first.
        assert!(items[0]["timestamp"].as_i64().unwrap() >= items[1]["timestamp"].as_i64().unwrap());
    }

    #[tokio::test]
    async fn list_proxy_logs_filters_and_paginates() {
        let (router, now) = setup_with_events().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!(
                "/api/data/proxy-log?start_ts={}&end_ts={}&model_name=gpt-4",
                now - 3600,
                now + 3600
            ),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json["total"], 2);
        assert!(
            resp.json["items"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["model_name"] == "gpt-4")
        );

        // Pagination: page 2 of per_page=1 -> the second most recent row.
        let resp = send_request(
            &router,
            Method::GET,
            &format!(
                "/api/data/proxy-log?start_ts={}&end_ts={}&per_page=1&page=2",
                now - 3600,
                now + 3600
            ),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json["total"], 3);
        assert_eq!(resp.json["page"], 2);
        assert_eq!(resp.json["per_page"], 1);
        assert_eq!(resp.json["items"].as_array().unwrap().len(), 1);

        // Status filter combined with model.
        let resp = send_request(
            &router,
            Method::GET,
            &format!(
                "/api/data/proxy-log?start_ts={}&end_ts={}&model_name=gpt-4&status=error",
                now - 3600,
                now + 3600
            ),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json["total"], 1);
        assert_eq!(resp.json["items"][0]["status"], "error");
    }

    #[tokio::test]
    async fn list_proxy_logs_rejects_invalid_timestamps() {
        let (router, _now) = setup_with_events().await;
        let resp = send_request(
            &router,
            Method::GET,
            "/api/data/proxy-log?start_ts=999999999999999999999",
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_proxy_logs_clamps_per_page() {
        let (router, now) = setup_with_events().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!(
                "/api/data/proxy-log?start_ts={}&end_ts={}&per_page=1000",
                now - 3600,
                now + 3600
            ),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json["per_page"], 100);
    }
}
