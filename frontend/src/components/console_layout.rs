use leptos::prelude::*;
use leptos_router::components::Outlet;

use crate::auth::{AuthContext, AuthStatus};
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
        if auth.authenticated.get() == AuthStatus::Authenticated {
            view! {
                <ConsoleLayout>
                    <Outlet />
                </ConsoleLayout>
            }
            .into_any()
        } else if auth.authenticated.get() == AuthStatus::NotAuthenticated {
            view! {
                <ConsoleLayout>
                    <div class="flex items-center justify-center h-full">
                        <div class="text-center">
                            <p class="text-gray-500 dark:text-gray-400 mb-4">"Not authenticated"</p>
                            <button
                                class="px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white text-sm font-medium rounded-lg cursor-pointer"
                                on:click=move |_| {
                                    web_sys::window().map(|w| w.location().reload());
                                }
                            >
                                "Reload"
                            </button>
                        </div>
                    </div>
                </ConsoleLayout>
            }
            .into_any()
        } else {
            render_loading().into_any()
        }
    }
}
