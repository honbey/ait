use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::Deserialize;

use crate::app::AppState;
use crate::db::{AuditEvent, Database, RequestId, SessionUser, User};
use crate::error::{AitError, forbidden, internal_error, not_found};

// Single-user mode with a full user model and user-level session and API key management.
pub fn create_user(db: &Database, username: &str, password: &str) -> Result<User, String> {
    let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)
        .map_err(|e| format!("Failed to hash password: {e}"))?;
    let user = User {
        username: username.to_string(),
        password_hash,
        api_keys: vec![],
        created_at: Default::default(),
        updated_at: Default::default(),
    };
    db.insert_user(user.clone())
        .map_err(|e| format!("Failed to create user: {e}"))?;
    Ok(user)
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Extension(request_id): Extension<RequestId>,
    Path(username): Path<String>,
    Json(input): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    // Can only change your own password
    if session.username != username {
        return Err(forbidden("You can only change your own password"));
    }

    #[derive(Debug)]
    enum ChangeError {
        NotFound(String),
        WrongPassword,
        Internal(String),
    }

    let db = state.db.clone();
    let username_clone = username.clone();
    let current_password = input.current_password.clone();
    let new_password = input.new_password.clone();
    crate::run_blocking(move || -> Result<User, ChangeError> {
        let mut user = db
            .get_user(&username_clone)
            .map_err(|e| ChangeError::Internal(e.to_string()))?
            .ok_or_else(|| ChangeError::NotFound(format!("User '{}' not found", username_clone)))?;

        let valid = bcrypt::verify(&current_password, &user.password_hash)
            .map_err(|e| ChangeError::Internal(e.to_string()))?;
        if !valid {
            return Err(ChangeError::WrongPassword);
        }

        let new_hash = bcrypt::hash(&new_password, bcrypt::DEFAULT_COST)
            .map_err(|e| ChangeError::Internal(e.to_string()))?;
        user.password_hash = new_hash;
        user.updated_at = Utc::now();
        db.update_user(&user)
            .map_err(|e| ChangeError::Internal(e.to_string()))?;
        // Invalidate all sessions (including the current one) so that
        // previously issued session cookies cannot authenticate anymore.
        db.delete_sessions_for_user(&username_clone)
            .map_err(|e| ChangeError::Internal(e.to_string()))?;
        Ok(user)
    })
    .await
    .map_err(internal_error)?
    .map_err(|e| match e {
        ChangeError::NotFound(msg) => not_found(msg),
        ChangeError::WrongPassword => forbidden("Current password is incorrect"),
        ChangeError::Internal(msg) => internal_error(msg),
    })?;

    // Drop cached sessions for this user too, otherwise the in-memory cache
    // would still authenticate old cookies for the rest of the cache TTL.
    state.session_cache.retain(|_, v| v.0.username != username);

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        request_id: request_id.0,
        username: session.username.clone(),
        action: "change_password".into(),
        resource: "user".into(),
        resource_id: username,
        detail: None,
    });

    Ok(Json(serde_json::json!({"ok": true})))
}
