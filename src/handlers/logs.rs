use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::app::AppState;
use crate::db::SessionUser;
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
    pub username: Option<String>,
    pub api_key_name: Option<String>,
    pub endpoint: Option<String>,
    pub is_streaming: Option<bool>,
    pub upstream_model: Option<String>,
    pub provider_type: Option<String>,
    pub client_ip: Option<String>,
}

const MAX_PER_PAGE: u64 = 100;

pub async fn list_proxy_logs(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
    Query(q): Query<ProxyLogQuery>,
) -> Result<Json<PaginatedResponse<ProxyLogEntryResponse>>, (StatusCode, Json<AitError>)> {
    let page = q.page.unwrap_or(1).max(1);
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
        username: q.username,
        api_key_name: q.api_key_name,
        endpoint: q.endpoint,
        is_streaming: q.is_streaming,
        upstream_model: q.upstream_model,
        provider_type: q.provider_type,
        client_ip: q.client_ip,
    };

    let result = state.log_manager.query_proxy_logs(params).await;

    Ok(Json(PaginatedResponse {
        items: result.items,
        total: result.total,
        page,
        per_page,
    }))
}
