use crate::error::internal_error;
use axum::{Extension, Json, extract::State, http::StatusCode};
use serde::Serialize;

use crate::app::AppState;
use crate::db::SessionUser;
use crate::error::AitError;

#[derive(Serialize)]
pub struct DashboardStats {
    pub provider_count: usize,
    pub model_count: usize,
    pub api_request_count: u64,
    pub token_consumption: u64,
}

pub async fn dashboard_stats(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
) -> Result<Json<DashboardStats>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let (provider_count, model_count) =
        crate::run_blocking(move || -> Result<(usize, usize), crate::db::DbError> {
            let p = db.count_providers()?;
            let m = db.count_models()?;
            Ok((p, m))
        })
        .await
        .map_err(internal_error)?;
    let api_request_count = state.log_manager.total_requests(7).await;
    let token_consumption = state.log_manager.total_tokens(7).await;

    Ok(Json(DashboardStats {
        provider_count,
        model_count,
        api_request_count,
        token_consumption,
    }))
}
