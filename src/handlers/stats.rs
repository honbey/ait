use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::SessionUser;
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
    Extension(_session): Extension<SessionUser>,
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
        .map_err(internal_error)?;
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
