use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;

use crate::error::{AitError, internal_error};
use crate::handlers::analytics::validate_ts_range;

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
}

pub async fn overview_stats(
    State(state): State<AppState>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<OverviewStats>, (StatusCode, Json<AitError>)> {
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

    async fn setup() -> Router {
        let (state, _dir) = create_test_state_fast_logs();
        let router = test_router(state);
        router
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
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }
}
