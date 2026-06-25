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
use crate::db::{AuditEvent, Permission, Session, UserRole};
use crate::error::{AitError, conflict, forbidden, internal_error, unauthorized};
use crate::handlers::users::create_user;
use crate::middleware::extract_session_key;

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

/// Build a `Set-Cookie` header value for the session key.
fn set_cookie_header(session_key: &str, max_age: i64) -> String {
    format!(
        "session_key={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        session_key, max_age
    )
}

pub async fn login(
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
            let _ = bcrypt::verify(&input.password, dummy_hash());
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
        username: input.username.clone(),
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

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: input.username.clone(),
        action: "login".into(),
        resource: "session".into(),
        resource_id: input.username.clone(),
        detail: None,
    });

    Ok((
        headers,
        Json(LoginResponse {
            ok: true,
            session_key,
            role: user.role,
        }),
    ))
}

pub async fn register(
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

    if state
        .db
        .get_user(&input.username)
        .map_err(|_| internal_error("Database error"))?
        .is_some()
    {
        return Err(conflict("Username already exists"));
    }

    create_user(
        &state.db,
        &input.username,
        &input.password,
        UserRole::User,
    )
    .map_err(internal_error)?;

    state.log_manager.log_audit(AuditEvent {
        timestamp: Utc::now(),
        username: input.username.clone(),
        action: "register".into(),
        resource: "user".into(),
        resource_id: input.username.clone(),
        detail: None,
    });

    Ok(Json(serde_json::json!({"ok": true})))
}

pub async fn logout(
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
        Ok(Some(session)) if !session.is_expired() => {
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

fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| bcrypt::hash("dummy", bcrypt::DEFAULT_COST).expect("dummy hash"))
}
