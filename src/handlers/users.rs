use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, Database, SessionUser, User};
use crate::error::{AitError, conflict, forbidden, internal_error, not_found};

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

#[derive(Serialize)]
pub struct UserInfoResponse {
    pub username: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<User> for UserInfoResponse {
    fn from(u: User) -> Self {
        UserInfoResponse {
            username: u.username,
            created_at: u.created_at.timestamp(),
            updated_at: u.updated_at.timestamp(),
        }
    }
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(_session): Extension<SessionUser>,
) -> Result<Json<Vec<UserInfoResponse>>, (StatusCode, Json<AitError>)> {
    let db = state.db.clone();
    let users = crate::run_blocking(move || db.list_users())
        .await
        .map_err(internal_error)?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    // Cannot delete yourself
    if session.username == username {
        return Err(conflict("Cannot delete yourself"));
    }

    let db = state.db.clone();
    let username_clone = username.clone();
    crate::run_blocking(move || -> Result<(), crate::db::DbError> {
        let _ = db.get_user(&username_clone).ok().flatten().ok_or_else(|| {
            crate::db::DbError::NotFound(format!("User '{}' not found", username_clone))
        })?;
        db.delete_user(&username_clone)?;
        Ok(())
    })
    .await
    .map_err(|e| match e {
        crate::db::DbError::NotFound(msg) => not_found(msg),
        crate::db::DbError::Storage(msg) => internal_error(msg),
        _ => internal_error(e.to_string()),
    })?;

    state.session_cache.clear();

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "delete".into(),
        resource: "user".into(),
        resource_id: username,
        detail: None,
    });

    Ok((StatusCode::NO_CONTENT,))
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
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
            .map_err(|e| ChangeError::Internal(e.to_string()))
    })
    .await
    .map_err(|e| match e {
        ChangeError::NotFound(msg) => not_found(msg),
        ChangeError::WrongPassword => forbidden("Current password is incorrect"),
        ChangeError::Internal(msg) => internal_error(msg),
    })?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "change_password".into(),
        resource: "user".into(),
        resource_id: username,
        detail: None,
    });

    Ok(Json(serde_json::json!({"ok": true})))
}
