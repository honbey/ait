use std::{sync::OnceLock, time::Instant};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, hash_key};
use crate::error::{AitError, internal_error, unauthorized};
use crate::middleware::{CACHE_TTL, extract_session_key};

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
        Ok(Ok((u, true))) => u,
        Ok(Ok((_u, false))) => return Err(unauthorized("Invalid credentials")),
        Ok(Err(Lookup::NoUsers)) => {
            return Err(unauthorized(
                "No users configured. Please check server configuration.",
            ));
        }
        Ok(Err(Lookup::InvalidCreds)) => return Err(unauthorized("Invalid credentials")),
        Ok(Err(Lookup::Db)) => return Err(internal_error("Database error")),
        Err(join_err) => return Err(internal_error(join_err)),
    };

    let ttl = state.config.auth.session_ttl_secs;
    let expires_at = Utc::now() + chrono::Duration::seconds(ttl as i64);

    let db = state.db.clone();
    let username = input.username.clone();
    let session_key = crate::run_blocking(move || db.insert_session(&username, expires_at))
        .await
        .map_err(internal_error)?
        .map_err(|e| AitError::from_db_error(e).into_response())?;

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

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<AitError>)> {
    if let Some(raw_key) = extract_session_key(&headers) {
        let hash = hash_key(raw_key);
        state.session_cache.remove(&hash);
        let db = state.db.clone();
        let session_key = raw_key.to_string();
        match crate::run_blocking(move || db.delete_session(&session_key)).await {
            Ok(_) | Err(_) => {}
        }
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
    let hash = hash_key(&session_key);

    if let Some(cached) = state.session_cache.get(&hash) {
        let (ref user, ref expires_at, ref inserted_at) = *cached;
        if *expires_at > Utc::now() && inserted_at.elapsed() < CACHE_TTL {
            return Json(SessionResponse {
                authenticated: true,
                username: Some(user.username.clone()),
            });
        }
    }

    let db = state.db.clone();
    let session = match crate::run_blocking(move || db.get_session(&session_key)).await {
        Ok(Ok(Some(s))) if !s.is_expired() => s,
        _ => {
            return Json(SessionResponse {
                authenticated: false,
                username: None,
            });
        }
    };

    let user = crate::db::SessionUser {
        username: session.username.clone(),
        api_key_name: None,
    };

    state
        .session_cache
        .insert(hash, (user, session.expires_at, Instant::now()));

    Json(SessionResponse {
        authenticated: true,
        username: Some(session.username),
    })
}

fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| bcrypt::hash("dummy", bcrypt::DEFAULT_COST).expect("dummy hash"))
}
