use leptos::prelude::*;
use leptos_router::components::Outlet;

use crate::components::sidebar::Sidebar;

#[component]
pub fn ConsoleLayout(children: Children) -> impl IntoView {
    view! {
        <div class="flex h-[calc(100vh-3.5rem)] overflow-hidden bg-gray-50 dark:bg-ink-950">
            <Sidebar />
            <div class="flex-1 p-8 overflow-y-auto">{children()}</div>
        </div>
    }
}

#[component]
pub fn ConsoleShell() -> impl IntoView {
    view! {
        <ConsoleLayout>
            <Outlet />
        </ConsoleLayout>
    }
}
