use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{Permission, User, UserRole};
use crate::error::AitError;

#[derive(Serialize)]
pub struct UserInfo {
    pub username: String,
    pub role: UserRole,
    pub allowed: Vec<Permission>,
    pub created_at: DateTime<Utc>,
}

impl From<User> for UserInfo {
    fn from(u: User) -> Self {
        UserInfo {
            username: u.username,
            role: u.role,
            allowed: u.allowed,
            created_at: u.created_at,
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

fn extract_session_key(headers: &HeaderMap) -> Option<&str> {
    // Check Authorization header first
    if let Some(key) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        return Some(key);
    }
    // Fall back to Cookie header
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("session_key=") {
            return Some(value);
        }
    }
    None
}

fn unauthorized(msg: &str) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(AitError {
            message: msg.to_string(),
            code: 401,
            r#type: "auth_error".to_string(),
        }),
    )
}

fn internal_error(msg: &str) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AitError {
            message: msg.to_string(),
            code: 500,
            r#type: "internal_error".to_string(),
        }),
    )
}

pub async fn list_users_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<UserInfo>>, (StatusCode, Json<AitError>)> {
    let users = state.db.list_users().map_err(|e| internal_error(&e))?;
    Ok(Json(users.into_iter().map(Into::into).collect()))
}

pub async fn update_user_handler(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Json(input): Json<UpdateUserRequest>,
) -> Result<Json<UserInfo>, (StatusCode, Json<AitError>)> {
    let mut user = match state.db.get_user(&username) {
        Ok(Some(u)) => u,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(AitError {
                    message: format!("User '{}' not found", username),
                    code: 404,
                    r#type: "not_found".to_string(),
                }),
            ));
        }
        Err(e) => return Err(internal_error(&e)),
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
        .map_err(|e| internal_error(&e))?;

    let updated = state
        .db
        .get_user(&username)
        .map_err(|e| internal_error(&e))?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AitError {
                    message: "User lost after update".to_string(),
                    code: 500,
                    r#type: "internal_error".to_string(),
                }),
            )
        })?;

    Ok(Json(updated.into()))
}

pub async fn delete_user_handler(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    state
        .db
        .delete_user(&username)
        .map_err(|e| internal_error(&e))?;
    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn change_password_handler(
    State(state): State<AppState>,
    Path(username): Path<String>,
    headers: HeaderMap,
    Json(input): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<AitError>)> {
    // Identify the caller via session
    let session_key =
        extract_session_key(&headers).ok_or_else(|| unauthorized("Authentication required"))?;
    let session = state
        .db
        .get_session(session_key)
        .map_err(|e| internal_error(&e))?
        .ok_or_else(|| unauthorized("Invalid session"))?;

    if session.expires_at <= Utc::now() {
        return Err(unauthorized("Session expired"));
    }

    // Only allow changing own password (role check deferred to middleware branch)
    if session.username != username {
        return Err((
            StatusCode::FORBIDDEN,
            Json(AitError {
                message: "You can only change your own password".to_string(),
                code: 403,
                r#type: "forbidden".to_string(),
            }),
        ));
    }

    let mut user = state
        .db
        .get_user(&username)
        .map_err(|e| internal_error(&e))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(AitError {
                    message: format!("User '{}' not found", username),
                    code: 404,
                    r#type: "not_found".to_string(),
                }),
            )
        })?;

    // Verify current password
    let valid = bcrypt::verify(&input.current_password, &user.password_hash)
        .map_err(|_| internal_error("Password verification error"))?;
    if !valid {
        return Err(unauthorized("Current password is incorrect"));
    }

    // Hash and set new password
    let new_hash = bcrypt::hash(&input.new_password, bcrypt::DEFAULT_COST)
        .map_err(|_| internal_error("Failed to hash password"))?;
    user.password_hash = new_hash;

    state
        .db
        .update_user(&username, user)
        .map_err(|e| internal_error(&e))?;

    Ok(Json(serde_json::json!({"ok": true})))
}
