use crate::api;
use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthStatus {
    Unknown,
    Authenticated,
    NotAuthenticated,
}

#[derive(Clone)]
pub struct AuthContext {
    pub authenticated: RwSignal<AuthStatus>,
    pub username: RwSignal<Option<String>>,
}

impl AuthContext {
    pub fn new() -> Self {
        Self {
            authenticated: RwSignal::new(AuthStatus::Unknown),
            username: RwSignal::new(None),
        }
    }

    pub fn set_logged_in(&self, uname: String) {
        self.authenticated.set(AuthStatus::Authenticated);
        self.username.set(Some(uname));
    }

    pub fn set_logged_out(&self) {
        api::clear_cache();
        self.authenticated.set(AuthStatus::NotAuthenticated);
        self.username.set(None);
    }
}
