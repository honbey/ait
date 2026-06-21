use sycamore::prelude::*;
use sycamore::web::create_client_resource;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::api::{fetch_api_keys, fetch_dashboard, fetch_models, fetch_providers};
use crate::i18n::I18n;
use crate::models::{ApiKeyListItem, DashboardData, Model, Provider};
use crate::route::Route;

use crate::components::sidebar::{Sidebar, SidebarProps};
use crate::components::topbar::{Topbar, TopbarProps};

pub fn render_loading() -> View {
    div()
        .class("min-h-screen bg-gray-50 dark:bg-gray-900 flex items-center justify-center")
        .children(
            div()
                .class("w-32 h-32 rounded-full bg-indigo-200 dark:bg-indigo-700")
                .children(
                    svg()
                        .class("animate-spin text-indigo-600 dark:text-indigo-400")
                        .xmlns("http://www.w3.org/2000/svg")
                        .fill("none")
                        .viewBox("0 0 24 24")
                        .children((
                            circle()
                                .class("opacity-25")
                                .cx("12")
                                .cy("12")
                                .r("10")
                                .stroke("currentColor")
                                .strokeWidth("4"),
                            path()
                                .class("opacity-75")
                                .fill("currentColor")
                                .d(
                                    "M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z",
                                ),
                        )),
                ),
        )
        .into()
}

pub fn render_error_view(i18n: &I18n, msg: String) -> View {
    div()
        .class("min-h-screen bg-gray-50 dark:bg-gray-900 flex items-center justify-center")
        .children(
            div()
                .class(
                    "bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 px-6 py-4 rounded-lg",
                )
                .children((
                    p().class("font-semibold").children(i18n.t("load_failed")),
                    p().class("text-sm mt-1").children(msg),
                )),
        )
        .into()
}

#[derive(Clone)]
enum RouteData {
    Dashboard(DashboardData),
    Providers(Vec<Provider>),
    Models(Vec<Model>, Vec<Provider>),
    ApiKeys(Vec<ApiKeyListItem>),
    TextGeneration(Vec<Model>),
    Placeholder,
    Error(String),
}

fn render_placeholder(i18n: &I18n, title_key: &str) -> View {
    let title = i18n.t(title_key);
    let suffix = i18n.t("in_development");
    div().children(
        div()
            .class("p-4 sm:p-8")
            .children(
                div()
                    .class(
                        "h-64 flex items-center justify-center bg-white dark:bg-gray-800 rounded-xl border-2 border-dashed border-gray-300 dark:border-gray-600",
                    )
                    .children(
                        span().class("text-gray-400 dark:text-gray-500 text-lg")
                            .children(format!("{} - {}", title, suffix)),
                    ),
            ),
    )
    .into()
}

#[derive(Props)]
pub struct LayoutProps {
    pub dark: Signal<bool>,
    pub route: Signal<Route>,
    pub authenticated: Signal<bool>,
    pub username: Signal<Option<String>>,
    pub role: Signal<Option<String>>,
}

#[component]
pub fn Layout(props: LayoutProps) -> View {
    let dark = props.dark;
    let route = props.route;
    let authenticated = props.authenticated;
    let username = props.username;
    let role = props.role;
    let sidebar_open = create_signal(false);
    let i18n = use_context::<I18n>();

    // Auth guard: redirect to Login if not authenticated
    create_effect(move || {
        if route.get().is_console() && !authenticated.get() {
            route.set(Route::Login);
        }
    });

    let provider_refresh = create_signal(0usize);
    let provider_refreshing = create_signal(false);
    let model_refresh = create_signal(0usize);
    let model_refreshing = create_signal(false);
    let api_key_refresh = create_signal(0usize);
    let api_key_refreshing = create_signal(false);
    let dep = create_memo(move || {
        (
            route.get(),
            provider_refresh.get(),
            model_refresh.get(),
            api_key_refresh.get(),
        )
    });
    let data = create_client_resource(on(dep, move || {
        let pr = provider_refreshing;
        let mr = model_refreshing;
        let ar = api_key_refreshing;
        let uname = username;
        async move {
            let result = match route.get() {
                Route::Dashboard => fetch_dashboard()
                    .await
                    .map(RouteData::Dashboard)
                    .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                Route::Providers => fetch_providers()
                    .await
                    .map(RouteData::Providers)
                    .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                Route::Models => {
                    let models = fetch_models().await;
                    let providers = fetch_providers().await;
                    match (models, providers) {
                        (Ok(m), Ok(p)) => RouteData::Models(m, p),
                        (Err(e), _) | (_, Err(e)) => RouteData::Error(e.to_string()),
                    }
                }
                Route::ApiKeys => match uname.get_clone() {
                    Some(u) => fetch_api_keys(&u)
                        .await
                        .map(RouteData::ApiKeys)
                        .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                    None => RouteData::Placeholder,
                },
                Route::TextGeneration => fetch_models()
                    .await
                    .map(RouteData::TextGeneration)
                    .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                _ => RouteData::Placeholder,
            };
            match route.get() {
                Route::Providers => pr.set(false),
                Route::Models => mr.set(false),
                Route::ApiKeys => ar.set(false),
                _ => {}
            }
            result
        }
    }));

    let i18n_view = i18n.clone();

    div()
        .id("app")
        .class(move || if dark.get() { "dark" } else { "" })
        .children(
            div()
                .class("min-h-screen bg-gray-50 dark:bg-gray-900")
                .children((
                    Topbar(TopbarProps { dark, route, authenticated, username }),
                    // Mobile overlay backdrop
                    View::from_dynamic(move || {
                        if route.get().is_console() {
                            div()
                                .class(move || {
                                    if sidebar_open.get() {
                                        "fixed inset-0 bg-black/50 z-30 md:hidden transition-opacity duration-300"
                                    } else {
                                        "fixed inset-0 bg-black/50 z-30 md:hidden opacity-0 pointer-events-none transition-opacity duration-300"
                                    }
                                })
                                .on(events::click, move |_| sidebar_open.set(false))
                                .into()
                        } else {
                            View::new()
                        }
                    }),
                    View::from_dynamic(move || {
                        if route.get().is_console() {
                            div().children((
                                Sidebar(SidebarProps { open: sidebar_open, route }),
                                button()
                                    .class(move || {
                                        "md:hidden fixed bottom-6 left-1/2 -translate-x-1/2 z-50 w-10 h-10 rounded-full bg-indigo-600 dark:bg-indigo-400 text-white shadow-lg flex items-center justify-center transition-all duration-200 opacity-30 hover:opacity-100"
                                    })
                                    .on(events::click, move |_| sidebar_open.set(!sidebar_open.get()))
                                    .children(
                                        i().class(move || {
                                            if sidebar_open.get() {
                                                "fas fa-chevron-left text-sm"
                                            } else {
                                                "fas fa-chevron-right text-sm"
                                            }
                                        }),
                                    ),
                            )).into()
                        } else {
                            View::new()
                        }
                    }),
                    div()
                        .class(move || {
                            if route.get().is_console() {
                                "md:ml-60 min-h-[calc(100vh-3.5rem)]"
                            } else {
                                "min-h-[calc(100vh-3.5rem)]"
                            }
                        })
                        .children(View::from_dynamic(move || {
                            match route.get() {
                                Route::Index => {
                                    crate::views::index::render_index_view(
                                        &i18n_view,
                                        route,
                                        authenticated,
                                    )
                                }
                                Route::Login => {
                                    crate::views::login::render_login_view(
                                        &i18n_view,
                                        authenticated,
                                        route,
                                        username,
                                        role,
                                    )
                                }
                                Route::Register => {
                                    crate::views::register::render_register_view(
                                        &i18n_view,
                                        route,
                                    )
                                }
                                _ => match data.get_clone() {
                                    Some(RouteData::Dashboard(d)) => {
                                        crate::views::dashboard::render_dashboard_view(&i18n_view, d)
                                    }
                                    Some(RouteData::Providers(p)) => {
                                        let is_admin = create_signal(role.get_clone().as_deref() == Some("admin"));
                                        crate::views::providers::render_providers_view(p, is_admin, provider_refresh, provider_refreshing)
                                    }
                                    Some(RouteData::Models(m, p)) => {
                                        let is_admin = create_signal(role.get_clone().as_deref() == Some("admin"));
                                        crate::views::models::render_models_view(m, p, is_admin, model_refresh, model_refreshing)
                                    }
                                    Some(RouteData::ApiKeys(k)) => {
                                        let uname = username.get_clone().unwrap_or_default();
                                        crate::views::api_keys::render_api_keys_view(k, uname, api_key_refresh, api_key_refreshing)
                                    }
                                    Some(RouteData::TextGeneration(m)) => {
                                        crate::views::text_generation::render_text_generation_view(m)
                                    }
                                    Some(RouteData::Placeholder) => match route.get() {
                                        Route::ApiKeys => {
                                            render_placeholder(&i18n_view, "api_key")
                                        }
                                        Route::TextGeneration => {
                                            render_placeholder(&i18n_view, "text_generation")
                                        }
                                        Route::Dashboard
                                        | Route::Providers
                                        | Route::Models => render_loading(),
                                        _ => render_loading(),
                                    },
                                    Some(RouteData::Error(e)) => {
                                        render_error_view(&i18n_view, e)
                                    }
                                    None => render_loading(),
                                },
                            }
                        })),
                )),
        )
        .into()
}
