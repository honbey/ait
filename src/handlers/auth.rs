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
use crate::db::{AuditEvent, Session, hash_key};
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
    #[derive(Debug)]
    enum Lookup {
        NoUsers,
        InvalidCreds,
        Db,
    }

    let db = state.db.clone();
    let username = input.username.clone();
    let password = input.password.clone();
    let _ = match crate::run_blocking(move || match db.get_user(&username) {
        Ok(Some(u)) => {
            let valid = bcrypt::verify(&password, &u.password_hash).unwrap_or(false);
            Ok((u, valid))
        }
        Ok(None) => {
            let _ = bcrypt::verify(&password, dummy_hash());
            if !db.has_any_users().unwrap_or(false) {
                return Err(Lookup::NoUsers);
            }
            Err(Lookup::InvalidCreds)
        }
        Err(_) => Err(Lookup::Db),
    })
    .await
    {
        Ok((u, true)) => u,
        Ok((_u, false)) => return Err(unauthorized("Invalid credentials")),
        Err(Lookup::NoUsers) => {
            return Err(unauthorized(
                "No users configured. Please check server configuration.",
            ));
        }
        Err(Lookup::InvalidCreds) => return Err(unauthorized("Invalid credentials")),
        Err(Lookup::Db) => return Err(internal_error("Database error")),
    };

    let session_key = generate_session_key();
    let ttl = state.config.auth.session_ttl_secs;
    let session = Session {
        session_key: session_key.clone(),
        username: input.username.clone(),
        created_at: Utc::now(),
        expires_at: Utc::now() + chrono::Duration::seconds(ttl as i64),
    };

    let db = state.db.clone();
    crate::run_blocking(move || db.insert_session(session))
        .await
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

    let db = state.db.clone();
    let username = input.username.clone();
    let password = input.password.clone();
    let exists = crate::run_blocking(move || {
        let user = db.get_user(&username).ok().flatten();
        if user.is_some() {
            let _ = bcrypt::verify(&password, dummy_hash());
        }
        user.is_some()
    })
    .await;
    if exists {
        return Err(conflict("Username already exists"));
    }

    let db = state.db.clone();
    let username = input.username.clone();
    let password = input.password.clone();
    crate::run_blocking(move || create_user(&db, &username, &password))
        .await
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
    if let Some(raw_key) = extract_session_key(&headers) {
        let hash = hash_key(raw_key);
        state.session_cache.remove(&hash);
        let db = state.db.clone();
        let session_key = raw_key.to_string();
        crate::run_blocking(move || db.delete_session(&session_key))
            .await
            .ok();
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
            });
        }
    };

    let session_key = session_key.to_string();
    // Fast single RocksDB get_cf (~10–50 µs); spawn_blocking overhead
    // (~5–20 µs) would exceed the work itself, so called directly.
    let result = (|| {
        let session = match state.db.get_session(&session_key) {
            Ok(Some(s)) if !s.is_expired() => s,
            _ => return Err(()),
        };
        let user = state.db.get_user(&session.username).ok().flatten();
        Ok((session, user))
    })();

    match result {
        Ok((session, Some(_u))) => Json(SessionResponse {
            authenticated: true,
            username: Some(session.username),
        }),
        _ => Json(SessionResponse {
            authenticated: false,
            username: None,
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
