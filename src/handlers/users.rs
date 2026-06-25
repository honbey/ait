use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, Database, Permission, SessionUser, User, UserRole};
use crate::error::{AitError, conflict, forbidden, internal_error, not_found, require_admin};

pub fn create_user(
    db: &Database,
    username: &str,
    password: &str,
    role: UserRole,
) -> Result<User, String> {
    let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)
        .map_err(|e| format!("Failed to hash password: {e}"))?;
    let user = User {
        username: username.to_string(),
        password_hash,
        role,
        allowed: vec![],
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
    pub role: UserRole,
    pub allowed: Vec<Permission>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<User> for UserInfoResponse {
    fn from(u: User) -> Self {
        UserInfoResponse {
            username: u.username,
            role: u.role,
            allowed: u.allowed,
            created_at: u.created_at.timestamp(),
            updated_at: u.updated_at.timestamp(),
        }
    }
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<UserRole>,
    pub allowed: Option<Vec<Permission>>,
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<UserInfoResponse>>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let users = state.db.list_users()?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub async fn update_user(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
    Json(input): Json<UpdateUserRequest>,
) -> Result<Json<UserInfoResponse>, (StatusCode, Json<AitError>)> {
    require_admin(&session)?;
    let mut user = state
        .db
        .get_user(&username)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    if let Some(role) = input.role {
        // Prevent demoting the last admin
        if role != UserRole::Admin
            && user.role == UserRole::Admin
            && state.db.count_admins().map_err(internal_error)? <= 1
        {
            return Err(conflict("Cannot demote the last admin"));
        }
        user.role = role;
    }
    if let Some(allowed) = input.allowed {
        user.allowed = allowed;
    }

    let updated = state.db.update_user(&user)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: session.username.clone(),
        action: "update".into(),
        resource: "user".into(),
        resource_id: username,
        detail: None,
    });

    Ok(Json(updated.into()))
}

pub async fn delete_user(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<(StatusCode,), (StatusCode, Json<AitError>)> {
    require_admin(&session)?;

    // Cannot delete yourself
    if session.username == username {
        return Err(conflict("Cannot delete yourself"));
    }

    let user = state
        .db
        .get_user(&username)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    // Prevent deleting the last admin
    if user.role == UserRole::Admin && state.db.count_admins().map_err(internal_error)? <= 1 {
        return Err(conflict("Cannot delete the last admin"));
    }

    state.db.delete_user(&username)?;

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
    // Can only change own password, unless Admin
    if session.role != UserRole::Admin && session.username != username {
        return Err(forbidden("You can only change your own password"));
    }

    let mut user = state
        .db
        .get_user(&username)
        .map_err(internal_error)?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    // Verify current password when changing own password (all roles)
    if session.username == username {
        let valid = bcrypt::verify(&input.current_password, &user.password_hash)
            .map_err(|_| internal_error("Password verification error"))?;
        if !valid {
            return Err(forbidden("Current password is incorrect"));
        }
    }

    let new_hash = bcrypt::hash(&input.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| internal_error("Failed to hash password"))?;
    user.password_hash = new_hash;

    state.db.update_user(&user)?;

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
