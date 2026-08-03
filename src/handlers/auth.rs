use std::{sync::OnceLock, time::Instant};

use axum::{
    Extension, Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::db::{AuditEvent, RequestId, hash_key};
use crate::error::{AitError, internal_error, unauthorized};
use crate::middleware::{CACHE_TTL, extract_session_key, set_cookie_header};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub ok: bool,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub username: Option<String>,
}

pub async fn login(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
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
        request_id: request_id.0,
        username: input.username.clone(),
        action: "login".into(),
        resource: "session".into(),
        resource_id: input.username.clone(),
        detail: None,
    });

    Ok((headers, Json(LoginResponse { ok: true })))
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

pub async fn session_check(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let session_key = match extract_session_key(&headers) {
        Some(k) => k,
        None => {
            return (
                HeaderMap::new(),
                Json(SessionResponse {
                    authenticated: false,
                    username: None,
                }),
            );
        }
    };

    let session_key = session_key.to_string();
    let hash = hash_key(&session_key);
    let ttl = state.config.auth.session_ttl_secs;

    if let Some(mut entry) = state.session_cache.get_mut(&hash) {
        if entry.1 > Utc::now() && entry.2.elapsed() < CACHE_TTL {
            entry.2 = Instant::now();
            let user = entry.0.clone();
            drop(entry);
            let db = state.db.clone();
            let hash_clone = hash.clone();
            tokio::spawn(async move {
                let _ = crate::run_blocking(move || db.renew_session(&hash_clone, ttl)).await;
            });
            let mut resp_headers = HeaderMap::new();
            resp_headers.insert(
                header::SET_COOKIE,
                set_cookie_header(&session_key, ttl as i64).parse().unwrap(),
            );
            return (
                resp_headers,
                Json(SessionResponse {
                    authenticated: true,
                    username: Some(user.username),
                }),
            );
        }
        drop(entry);
    }

    let session_key_for_db = session_key.clone();
    let db = state.db.clone();
    let session = match crate::run_blocking(move || db.get_session(&session_key_for_db)).await {
        Ok(Ok(Some(s))) if !s.is_expired() => s,
        _ => {
            return (
                HeaderMap::new(),
                Json(SessionResponse {
                    authenticated: false,
                    username: None,
                }),
            );
        }
    };

    // Fire-and-forget session renewal so active sessions do not expire
    let db = state.db.clone();
    let hash_clone = hash.clone();
    tokio::spawn(async move {
        let _ = crate::run_blocking(move || db.renew_session(&hash_clone, ttl)).await;
    });

    let user = crate::db::SessionUser {
        username: session.username.clone(),
        api_key_name: None,
    };

    state
        .session_cache
        .insert(hash, (user, session.expires_at, Instant::now()));

    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(
        header::SET_COOKIE,
        set_cookie_header(&session_key, ttl as i64).parse().unwrap(),
    );

    (
        resp_headers,
        Json(SessionResponse {
            authenticated: true,
            username: Some(session.username),
        }),
    )
}

fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| bcrypt::hash("dummy", bcrypt::DEFAULT_COST).expect("dummy hash"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        create_test_state, insert_test_user, login_and_cookie, send_request, test_router,
    };
    use axum::Router;
    use axum::http::Method;

    async fn setup() -> (Router, String) {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let cookie = login_and_cookie(&router, "alice", "secret123").await;
        (router, cookie)
    }

    #[tokio::test]
    async fn login_success_sets_cookie_and_ok() {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/auth/login",
            None,
            None,
            Some(serde_json::json!({"username": "alice", "password": "secret123"})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["ok"], serde_json::Value::Bool(true));
        let cookie = resp
            .headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .expect("login should set a session cookie");
        assert!(cookie.starts_with("session_key="));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
    }

    #[tokio::test]
    async fn login_wrong_password_unauthorized() {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/auth/login",
            None,
            None,
            Some(serde_json::json!({"username": "alice", "password": "wrong"})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
        assert_eq!(resp.json["code"], 401);
    }

    #[tokio::test]
    async fn login_unknown_user_unauthorized() {
        let (state, _dir) = create_test_state();
        insert_test_user(&state.db, "alice", "secret123");
        let router = test_router(state);
        let resp = send_request(
            &router,
            Method::POST,
            "/auth/login",
            None,
            None,
            Some(serde_json::json!({"username": "nobody", "password": "secret123"})),
        )
        .await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
        assert_eq!(resp.json["code"], 401);
    }

    #[tokio::test]
    async fn session_check_with_cookie_authenticated() {
        let (router, cookie) = setup().await;
        let resp = send_request(
            &router,
            Method::GET,
            "/auth/session",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["authenticated"], serde_json::Value::Bool(true));
        assert_eq!(resp.json["username"], "alice");
    }

    #[tokio::test]
    async fn session_check_without_cookie_unauthenticated() {
        let (router, _cookie) = setup().await;
        let resp = send_request(&router, Method::GET, "/auth/session", None, None, None).await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.json["authenticated"], serde_json::Value::Bool(false));
    }

    #[tokio::test]
    async fn logout_invalidates_session() {
        let (router, cookie) = setup().await;
        let resp = send_request(
            &router,
            Method::POST,
            "/auth/logout",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.status, StatusCode::OK);
        let clear = resp
            .headers
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .expect("logout should clear the session cookie");
        assert!(clear.contains("Max-Age=0"));
        let resp = send_request(
            &router,
            Method::GET,
            "/auth/session",
            Some(&cookie),
            None,
            None,
        )
        .await;
        assert_eq!(resp.json["authenticated"], serde_json::Value::Bool(false));
    }
}
