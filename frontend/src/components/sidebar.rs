use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::i18n::{I18n, K};
use crate::route::AppRoute;

// ─── NavItem ───────────────────────────────────────────────────

#[derive(Props)]
pub struct NavItemProps {
    pub icon: &'static str,
    pub label_key: K,
    pub href: &'static str,
    pub app_route: AppRoute,
}

#[component]
fn NavItem(props: NavItemProps) -> View {
    let route = use_context::<ReadSignal<AppRoute>>();
    let app_route = props.app_route;
    let active = move || route.get() == app_route;
    let class = move || {
        if active() {
            "flex items-center gap-3 px-6 py-3 text-indigo-600 dark:text-indigo-400 bg-indigo-50 dark:bg-indigo-900/30 border-l-4 border-indigo-600 dark:border-indigo-400 font-semibold cursor-pointer"
        } else {
            "flex items-center gap-3 px-6 py-3 text-gray-500 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors"
        }
    };
    let i18n = use_context::<I18n>();
    let label_key = props.label_key;

    a().attr("href", props.href)
        .class(class)
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
}

#[component]
pub fn Sidebar(props: SidebarProps) -> View {
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
            nav().class("flex-1 py-4")
                .children((
                    NavItem(NavItemProps {
                        icon: "fa-chart-pie",
                        label_key: K::Dashboard,
                        href: "/console/dashboard",
                        app_route: AppRoute::Dashboard,
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-server",
                        label_key: K::Providers,
                        href: "/console/providers",
                        app_route: AppRoute::Providers,
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-cube",
                        label_key: K::Models,
                        href: "/console/models",
                        app_route: AppRoute::Models,
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-key",
                        label_key: K::ApiKey,
                        href: "/console/api-keys",
                        app_route: AppRoute::ApiKeys,
                    }),
                    NavItem(NavItemProps {
                        icon: "fa-comment-dots",
                        label_key: K::TextGeneration,
                        href: "/console/text-generation",
                        app_route: AppRoute::TextGeneration,
                    }),
                )),
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
