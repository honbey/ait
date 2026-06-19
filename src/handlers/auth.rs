use std::sync::OnceLock;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{Session, User, UserRole, Permission};
use crate::error::AitError;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub ok: bool,
    pub session_key: String,
    pub role: UserRole,
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub registration_code: String,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub username: Option<String>,
    pub role: Option<UserRole>,
    pub allowed: Option<Vec<Permission>>,
}

/// Extract `session_key` from the Cookie header.
fn extract_session_key(headers: &HeaderMap) -> Option<&str> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for pair in cookie.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("session_key=") {
            return Some(value);
        }
    }
    None
}

/// Build a `Set-Cookie` header value for the session key.
fn set_cookie_header(session_key: &str, max_age: i64) -> String {
    format!(
        "session_key={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        session_key, max_age
    )
}

pub async fn login_handler(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<AitError>)> {
    let user = match state.db.get_user(&input.username) {
        Ok(Some(u)) => u,
        Ok(None) => {
            let user_count = state.db.list_users().map(|v| v.len()).unwrap_or(0);
            if user_count == 0 {
                return Err(unauthorized(
                    "No users configured. Please check server configuration.",
                ));
            }
            // Constant-time comparison: always bcrypt verify to prevent timing side-channel
            let _ = bcrypt::verify(&input.password, &dummy_hash());
            return Err(unauthorized("Invalid credentials"));
        }
        Err(_) => return Err(internal_error("Database error")),
    };

    let valid = bcrypt::verify(&input.password, &user.password_hash)
        .map_err(|_| internal_error("Password verification error"))?;

    if !valid {
        return Err(unauthorized("Invalid credentials"));
    }

    let session_key = generate_session_key();
    let ttl = state.config.auth.session_ttl_secs;
    let session = Session {
        session_key: session_key.clone(),
        username: input.username,
        created_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::seconds(ttl as i64),
    };

    state
        .db
        .insert_session(session)
        .map_err(|_| internal_error("Failed to create session"))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        set_cookie_header(&session_key, ttl as i64).parse().unwrap(),
    );

    Ok((
        headers,
        Json(LoginResponse {
            ok: true,
            session_key,
            role: user.role,
        }),
    ))
}

pub async fn register_handler(
    State(state): State<AppState>,
    Json(input): Json<RegisterRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<AitError>)> {
    if !state.config.auth.allow_registration {
        return Err(forbidden("Registration is disabled"));
    }

    if !state.config.auth.registration_code.is_empty()
        && state.config.auth.registration_code != input.registration_code
    {
        return Err(forbidden("Invalid registration code"));
    }

    if state.db.get_user(&input.username).map_err(|_| internal_error("Database error"))?.is_some() {
        return Err(conflict("Username already exists"));
    }

    let password_hash = bcrypt::hash(&input.password, bcrypt::DEFAULT_COST)
        .map_err(|_| internal_error("Failed to hash password"))?;

    let user = User {
        username: input.username,
        password_hash,
        role: UserRole::User,
        allowed: vec![],
        api_keys: vec![],
        created_at: chrono::Utc::now(),
    };

    state.db.insert_user(user).map_err(|_| internal_error("Failed to create user"))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn logout_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<AitError>)> {
    if let Some(session_key) = extract_session_key(&headers) {
        state.db.delete_session(session_key).ok();
    }

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::SET_COOKIE,
        set_cookie_header("", 0).parse().unwrap(),
    );

    Ok((resp_headers, Json(serde_json::json!({"ok": true}))))
}

pub async fn session_check(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Json<SessionResponse> {
    let session_key = match extract_session_key(&headers) {
        Some(k) => k,
        None => {
            return Json(SessionResponse {
                authenticated: false,
                username: None,
                role: None,
                allowed: None,
            });
        }
    };

    match state.db.get_session(session_key) {
        Ok(Some(session)) if session.expires_at > Utc::now() => {
            let user = state.db.get_user(&session.username).ok().flatten();
            match user {
                Some(u) => Json(SessionResponse {
                    authenticated: true,
                    username: Some(session.username),
                    role: Some(u.role),
                    allowed: Some(u.allowed),
                }),
                None => Json(SessionResponse {
                    authenticated: false,
                    username: None,
                    role: None,
                    allowed: None,
                }),
            }
        }
        _ => Json(SessionResponse {
            authenticated: false,
            username: None,
            role: None,
            allowed: None,
        }),
    }
}

fn generate_session_key() -> String {
    use rand::RngExt;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

fn dummy_hash() -> String {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        bcrypt::hash("dummy", bcrypt::DEFAULT_COST).expect("dummy hash")
    }).clone()
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

fn forbidden(msg: &str) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::FORBIDDEN,
        Json(AitError {
            message: msg.to_string(),
            code: 403,
            r#type: "auth_error".to_string(),
        }),
    )
}

fn conflict(msg: &str) -> (StatusCode, Json<AitError>) {
    (
        StatusCode::CONFLICT,
        Json(AitError {
            message: msg.to_string(),
            code: 409,
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
