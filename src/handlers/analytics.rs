use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::SessionUser;
use crate::db::models::{BucketEntry, ModelDistEntry, TokenDistEntry};
use crate::error::AitError;

#[derive(Deserialize)]
pub struct AnalyticsQuery {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

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

    let (start, end) = if start >= end {
        (end - 86400, end)
    } else if end - start > max_range {
        (end - max_range, end)
    } else if end > now + 3600 {
        (start, now + 3600)
    } else {
        (start, end)
    };

    Ok(ValidatedRange { start, end })
}

pub async fn requests(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<BucketEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.requests(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn tokens(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<BucketEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.tokens(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn model_dist(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<ModelDistEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.model_dist(range.start, range.end).await;
    Ok(Json(result))
}

pub async fn token_dist(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Query(q): Query<AnalyticsQuery>,
) -> Result<Json<Vec<TokenDistEntry>>, (StatusCode, Json<AitError>)> {
    let range = validate_ts_range(q.start_ts, q.end_ts, state.config.log.retention_days)?;
    let result = state.log_manager.token_dist(range.start, range.end).await;
    Ok(Json(result))
}
