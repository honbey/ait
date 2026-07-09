use leptos::prelude::*;
use leptos_router::components::{Outlet, Redirect};

use crate::auth::AuthContext;
use crate::components::sidebar::Sidebar;
use crate::components::skeleton::render_loading;

#[component]
pub fn ConsoleLayout(children: Children) -> impl IntoView {
    view! {
        <div class="flex h-[calc(100vh-3.5rem)] overflow-hidden bg-gray-50 dark:bg-gray-900">
            <Sidebar />
            <div class="flex-1 p-8 overflow-y-auto">{children()}</div>
        </div>
    }
}

#[component]
pub fn ConsoleShell() -> impl IntoView {
    let auth = use_context::<AuthContext>().expect("AuthContext not provided");

    move || {
        if auth.authenticated.get() == Some(true) {
            view! {
                <ConsoleLayout>
                    <Outlet />
                </ConsoleLayout>
            }
            .into_any()
        } else if auth.authenticated.get() == Some(false) {
            view! { <Redirect path="/login" /> }.into_any()
        } else {
            render_loading().into_any()
        }
    }
}
