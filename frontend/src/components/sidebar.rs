use leptos::prelude::*;
use leptos_router::components::A;

use crate::t;

#[component]
fn NavItem(
    icon: &'static str,
    label: impl Fn() -> String + Send + 'static,
    href: &'static str,
) -> impl IntoView {
    view! {
        <A
            href=href
            exact=true
            {..}
            class="flex items-center gap-3 px-6 py-3 text-gray-500 font-semibold dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors aria-[current=page]:text-indigo-600 dark:aria-[current=page]:text-indigo-400 aria-[current=page]:bg-indigo-50 dark:aria-[current=page]:bg-indigo-900/30 aria-[current=page]:border-l-4 aria-[current=page]:border-indigo-600 dark:aria-[current=page]:border-indigo-400"
        >
            <i class=format!("fas {} w-5 text-center", icon)></i>
            <span>{move || label()}</span>
        </A>
    }
}

#[component]
pub fn Sidebar() -> impl IntoView {
    view! {
        <aside class="w-56 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col shrink-0">
            <nav class="flex-1 py-4 space-y-1 overflow-y-auto">
                <NavItem icon="fa-chart-pie" label=t!(Overview) href="/console" />
                <NavItem icon="fa-server" label=t!(Providers) href="/console/providers" />
                <NavItem icon="fa-cube" label=t!(Models) href="/console/models" />
                <NavItem icon="fa-key" label=t!(ApiKey) href="/console/apikeys" />
                <NavItem icon="fa-list" label=t!(LogQuery) href="/console/logs" />
                <NavItem
                    icon="fa-comment"
                    label=t!(TextGeneration)
                    href="/console/text-generation"
                />
            </nav>
            <div class="p-4 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-400 dark:text-gray-500">
                <p>{format!("Ait v{}", env!("CARGO_PKG_VERSION"))}</p>
            </div>
        </aside>
    }
}
