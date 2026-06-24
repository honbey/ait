use axum::{Extension, Json, extract::{Query, State}, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::models::{DailyRequests, DailyTokens};
use crate::db::SessionUser;
use crate::error::{AitError, require_admin};

#[derive(Deserialize)]
pub struct DaysQuery {
    pub days: Option<i64>,
}

#[derive(Serialize)]
pub struct DashboardStats {
    pub provider_count: usize,
    pub model_count: usize,
    pub api_request_count: u64,
    pub token_consumption: u64,
}

pub async fn daily_requests(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Query(q): Query<DaysQuery>,
) -> Result<Json<Vec<DailyRequests>>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let days = q.days.unwrap_or(7);
    Ok(Json(state.log_manager.daily_requests(days).await))
}

pub async fn daily_tokens(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Query(q): Query<DaysQuery>,
) -> Result<Json<Vec<DailyTokens>>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let days = q.days.unwrap_or(7);
    Ok(Json(state.log_manager.daily_tokens(days).await))
}

pub async fn dashboard_stats(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<DashboardStats>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;

    let providers = state.db.list_providers()?;
    let models = state.db.list_models()?;
    let api_request_count = state.log_manager.total_requests(7).await;
    let token_consumption = state.log_manager.total_tokens(7).await;

    Ok(Json(DashboardStats {
        provider_count: providers.len(),
        model_count: models.len(),
        api_request_count,
        token_consumption,
    }))
}
