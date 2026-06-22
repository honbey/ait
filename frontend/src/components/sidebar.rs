use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::i18n::{I18n, K};
use crate::route::Route;

// ─── NavItem ───────────────────────────────────────────────────

#[derive(Props)]
pub struct NavItemProps {
    pub icon: String,
    pub label_key: K,
    pub route: Route,
    pub current_route: Signal<Route>,
    pub i18n: I18n,
}

#[component]
fn NavItem(props: NavItemProps) -> View {
    let active = move || props.current_route.get() == props.route;
    let class = move || {
        if active() {
            "flex items-center gap-3 px-6 py-3 text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/30 border-l-4 border-indigo-600 dark:border-indigo-400 font-semibold cursor-pointer"
        } else {
            "flex items-center gap-3 px-6 py-3 text-gray-500 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors"
        }
    };
    let i18n = props.i18n;
    let label_key = props.label_key;

    div()
        .class(class)
        .on(events::click, move |_| props.current_route.set(props.route))
        .children((
            i().class(format!("fas {} w-5 text-center", props.icon)),
            span().children(View::from_dynamic(move || i18n.t(label_key))),
        ))
        .into()
}

// ─── Sidebar ───────────────────────────────────────────────────

#[derive(Props)]
pub struct SidebarProps {
    pub open: Signal<bool>,
    pub route: Signal<Route>,
}

#[component]
pub fn Sidebar(props: SidebarProps) -> View {
    let i18n = use_context::<I18n>();
    let sidebar_class = move || {
        let base = "w-60 h-[calc(100dvh-3.5rem)] fixed top-14 left-0 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col z-40 transition-transform duration-300 ease-in-out shadow-lg md:shadow-none";
        if props.open.get() {
            format!("{} translate-x-0", base)
        } else {
            format!("{} -translate-x-full md:translate-x-0", base)
        }
    };

    aside()
        .class(sidebar_class)
        .children((
            // Middle: Navigation links
            nav().class("flex-1 py-4")
                .children((
                    NavItem(NavItemProps {
                        icon: "fa-chart-pie".to_string(),
                        label_key: K::Dashboard,
                        route: Route::Dashboard,
                        current_route: props.route,
                        i18n: i18n.clone(),
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-server".to_string(),
                        label_key: K::Providers,
                        route: Route::Providers,
                        current_route: props.route,
                        i18n: i18n.clone(),
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-cube".to_string(),
                        label_key: K::Models,
                        route: Route::Models,
                        current_route: props.route,
                        i18n: i18n.clone(),
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-key".to_string(),
                        label_key: K::ApiKey,
                        route: Route::ApiKeys,
                        current_route: props.route,
                        i18n: i18n.clone(),
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-comment-dots".to_string(),
                        label_key: K::TextGeneration,
                        route: Route::TextGeneration,
                        current_route: props.route,
                        i18n: i18n.clone(),
                    }),
                )),
            // Bottom: Project info
            div()
                .class(
                    "p-4 border-t border-gray-200 dark:border-gray-700 text-xs text-gray-400 dark:text-gray-500",
                )
                .children(
                    p().children(format!("Ait v{}", env!("CARGO_PKG_VERSION"))),
                ),
        ))
        .into()
}
