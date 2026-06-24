use axum::{Extension, Json, extract::State, http::StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::db::SessionUser;
use crate::error::{AitError, require_admin};

#[derive(Serialize)]
pub struct DashboardStats {
    pub provider_count: usize,
    pub model_count: usize,
    pub api_request_count: u64,
    pub token_consumption: u64,
}

pub async fn dashboard_stats(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<DashboardStats>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;

    let providers = state.db.list_providers()?;
    let models = state.db.list_models()?;
    // let api_request_count = state.log_manager.requests_last_7d();
    let (api_request_count, token_consumption) = state.log_manager.query_stats().await;

    Ok(Json(DashboardStats {
        provider_count: providers.len(),
        model_count: models.len(),
        api_request_count,
        token_consumption,
    }))
}
