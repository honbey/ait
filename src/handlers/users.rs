use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{Permission, User, UserRole};
use crate::error::{internal_error, not_found, forbidden, forbidden_msg, AitError};
use crate::middleware::SessionUser;

#[derive(Serialize)]
pub struct UserInfo {
    pub username: String,
    pub role: UserRole,
    pub allowed: Vec<Permission>,
    pub created_at: i64,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        UserInfo {
            username: u.username,
            role: u.role,
            allowed: u.allowed,
            created_at: u.created_at.timestamp(),
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

pub async fn list_users_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
) -> Result<Json<Vec<UserInfo>>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let users = state.db.list_users().map_err(internal_error)?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub async fn update_user_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
    Json(input): Json<UpdateUserRequest>,
) -> Result<Json<UserInfo>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    let mut user = match state.db.get_user(&username) {
        Ok(Some(u)) => u,
        Ok(None) => return Err(not_found(format!("User '{}' not found", username))),
        Err(e) => return Err(internal_error(e)),
    };

    if let Some(role) = input.role {
        user.role = role;
    }
    if let Some(allowed) = input.allowed {
        user.allowed = allowed;
    }

    state
        .db
        .update_user(&username, user)
        .map_err(internal_error)?;

    let updated = state.db.get_user(&username).map_err(internal_error)?
        .ok_or_else(|| internal_error("User lost after update"))?;

    Ok(Json(updated.into()))
}

pub async fn delete_user_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    if session.role != UserRole::Admin {
        return Err(forbidden());
    }
    state.db.delete_user(&username).map_err(internal_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn change_password_handler(
    State(state): State<AppState>,
    Extension(session): Extension<SessionUser>,
    Path(username): Path<String>,
    Json(input): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    // Can only change own password, unless Admin
    if session.role != UserRole::Admin && session.username != username {
        return Err(forbidden_msg("You can only change your own password"));
    }

    let mut user = state.db.get_user(&username).map_err(internal_error)?
        .ok_or_else(|| not_found(format!("User '{}' not found", username)))?;

    // Verify current password when changing own password (all roles)
    if session.username == username {
        let valid = bcrypt::verify(&input.current_password, &user.password_hash)
            .map_err(|_| internal_error("Password verification error"))?;
        if !valid {
            return Err(forbidden_msg("Current password is incorrect"));
        }
    }

    let new_hash = bcrypt::hash(&input.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| internal_error("Failed to hash password"))?;
    user.password_hash = new_hash;

    state.db.update_user(&username, user).map_err(internal_error)?;
    Ok(Json(serde_json::json!({"ok": true})))
}
