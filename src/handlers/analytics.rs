use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;

use crate::db::models::{BucketEntry, ModelDistEntry, TokenDistEntry};
use crate::error::AitError;

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

#[derive(Debug)]
pub struct ValidatedRange {
    pub start: i64,
    pub end: i64,
}

/// Normalize timestamp range for analytics queries (tolerant mode).
/// Defaults: start = now - 30d, end = now.
/// Range upper bound = (retention_days + 1) * 86400 seconds.
pub fn validate_ts_range(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    retention_days: u64,
) -> Result<ValidatedRange, (StatusCode, Json<AitError>)> {
    let now = Utc::now().timestamp();

    let start = start_ts.unwrap_or(now - 30 * 86400);
    let end = end_ts.unwrap_or(now);

    if DateTime::from_timestamp(start, 0).is_none() {
        return Err(AitError::bad_request("invalid start_ts").into_response());
    }
    if DateTime::from_timestamp(end, 0).is_none() {
        return Err(AitError::bad_request("invalid end_ts").into_response());
    }

    let max_range = (retention_days as i64 + 1) * 86400;

    // Clamp `end` on its own so the bound holds whichever range correction
    // below applies; folding it into the chain let a future `end` through
    // whenever `start >= end` short-circuited on the first arm.
    let end = end.min(now + 3600);

    let (start, end) = if start >= end {
        (end - 86400, end)
    } else if end - start > max_range {
        (end - max_range, end)
    } else {
        (start, end)
    };

    Ok(ValidatedRange { start, end })
}

pub async fn requests(
    State(state): State<AppState>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<BucketEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.requests(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn tokens(
    State(state): State<AppState>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<BucketEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.tokens(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn model_dist(
    State(state): State<AppState>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<ModelDistEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.model_dist(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn token_dist(
    State(state): State<AppState>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<TokenDistEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.token_dist(range.start, range.end).await;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_last_30_days() {
        let range = validate_ts_range(None, None, 30).unwrap();
        let now = Utc::now().timestamp();
        assert_eq!(range.end, now);
        assert_eq!(range.start, now - 30 * 86400);
    }

    #[test]
    fn start_after_end_normalizes_to_one_day() {
        let now = Utc::now().timestamp();
        let range = validate_ts_range(Some(now), Some(now - 86400), 30).unwrap();
        assert_eq!(range.end, now - 86400);
        assert_eq!(range.start, range.end - 86400);
    }

    #[test]
    fn range_exceeding_retention_is_clamped() {
        let now = Utc::now().timestamp();
        let range = validate_ts_range(Some(now - 90 * 86400), Some(now), 30).unwrap();
        assert_eq!(range.end, now);
        assert_eq!(range.start, now - (30 + 1) * 86400);
    }

    #[test]
    fn future_end_is_clamped_to_now() {
        let now = Utc::now().timestamp();
        let range = validate_ts_range(Some(now - 3600), Some(now + 7200), 30).unwrap();
        assert_eq!(range.end, now + 3600);
        assert_eq!(range.start, now - 3600);
    }

    #[test]
    fn future_end_is_clamped_even_when_start_after_end() {
        let now = Utc::now().timestamp();
        // Both bounds far in the future: the previous mutually-exclusive chain
        // short-circuited on `start >= end` and let `end` through unclamped.
        let range = validate_ts_range(Some(now + 10_000), Some(now + 20_000), 30).unwrap();
        assert_eq!(range.end, now + 3600);
        assert_eq!(range.start, now + 3600 - 86400);
    }

    #[test]
    fn valid_range_passes_through() {
        let now = Utc::now().timestamp();
        let range = validate_ts_range(Some(now - 7200), Some(now - 3600), 30).unwrap();
        assert_eq!(range.start, now - 7200);
        assert_eq!(range.end, now - 3600);
    }

    #[test]
    fn invalid_timestamps_rejected() {
        let err = validate_ts_range(Some(i64::MAX), None, 30).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let err = validate_ts_range(None, Some(i64::MIN), 30).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use crate::test_utils::{
        create_test_state_fast_logs, make_proxy_event, send_request, test_router,
    };
    use axum::Router;
    use axum::http::Method;

    async fn setup() -> (Router, i64) {
        let (state, _dir) = create_test_state_fast_logs();
        let now = Utc::now().timestamp();
        let h = now / 3600 * 3600;
        // Two events in the current hour, one in the previous hour.
        let mut e1 = make_proxy_event("gpt-4", "success", 300);
        e1.timestamp = chrono::DateTime::from_timestamp(h + 100, 0).unwrap();
        let mut e2 = make_proxy_event("gpt-4", "success", 100);
        e2.timestamp = chrono::DateTime::from_timestamp(h + 2000, 0).unwrap();
        let mut e3 = make_proxy_event("llama", "error", 50);
        e3.timestamp = chrono::DateTime::from_timestamp(h - 3600 + 500, 0).unwrap();
        state.log_manager.log_proxy(e1);
        state.log_manager.log_proxy(e2);
        state.log_manager.log_proxy(e3);
        // Wait for all three events to be flushed.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if state.log_manager.total_requests(h - 7200, h + 7200).await >= 3 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for proxy events"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let router = test_router(state);
        (router, h)
    }

    fn range_qs(h: i64) -> String {
        format!("start_ts={}&end_ts={}", h - 7200, h + 7200)
    }

    #[tokio::test]
    async fn requests_endpoint_returns_hourly_buckets() {
        let (router, h) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/data/requests?{}", range_qs(h)),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let buckets = resp.json.as_array().unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0]["timestamp"], h - 3600);
        assert_eq!(buckets[0]["count"], 1);
        assert_eq!(buckets[1]["timestamp"], h);
        assert_eq!(buckets[1]["count"], 2);
    }

    #[tokio::test]
    async fn tokens_endpoint_sums_per_hour() {
        let (router, h) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/data/tokens?{}", range_qs(h)),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let buckets = resp.json.as_array().unwrap();
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[1]["count"], 400);
        assert_eq!(buckets[0]["count"], 50);
    }

    #[tokio::test]
    async fn model_dist_endpoint_groups_by_model() {
        let (router, h) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/data/model-dist?{}", range_qs(h)),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let dist = resp.json.as_array().unwrap();
        assert_eq!(dist.len(), 2);
        assert_eq!(dist[0]["model"], "gpt-4");
        assert_eq!(dist[0]["count"], 2);
        assert_eq!(dist[1]["model"], "llama");
        assert_eq!(dist[1]["count"], 1);
    }

    #[tokio::test]
    async fn token_dist_endpoint_returns_three_categories() {
        let (router, h) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            &format!("/api/data/token-dist?{}", range_qs(h)),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let dist = resp.json.as_array().unwrap();
        assert_eq!(dist.len(), 3);
        // Each event carries prompt=10, completion=20, cached=5: sums are
        // prompt=30, completion=60, cached=15.
        let get = |cat: &str| {
            dist.iter()
                .find(|e| e["category"] == cat)
                .map(|e| e["count"].as_u64().unwrap())
                .unwrap_or(0)
        };
        assert_eq!(get("uncached_input"), 15);
        assert_eq!(get("cached_input"), 15);
        assert_eq!(get("output"), 60);
    }
}
