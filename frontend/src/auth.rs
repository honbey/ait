use crate::api;
use leptos::prelude::*;

#[derive(Clone)]
pub struct AuthContext {
    pub authenticated: RwSignal<Option<bool>>,
    pub username: RwSignal<Option<String>>,
}

impl AuthContext {
    pub fn new() -> Self {
        Self {
            authenticated: RwSignal::new(None),
            username: RwSignal::new(None),
        }
    }

    pub fn set_logged_in(&self, uname: String) {
        self.authenticated.set(Some(true));
        self.username.set(Some(uname));
    }

    pub fn set_logged_out(&self) {
        api::clear_cache();
        self.authenticated.set(Some(false));
        self.username.set(None);
    }
}
