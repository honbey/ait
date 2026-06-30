use sycamore::prelude::*;
use sycamore::web::create_client_resource;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_router::navigate;

use crate::api::{
    fetch_api_keys, fetch_dashboard_stats, fetch_models, fetch_providers, fetch_request_buckets,
    fetch_token_buckets,
};
use crate::i18n::{I18n, K};
use crate::models::{ApiKeyListItem, DashboardData, Model, Provider};
use crate::route::AppRoute;

use crate::components::sidebar::{Sidebar, SidebarProps};
use crate::components::skeleton;
use crate::components::toast::render_toast_container;
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

pub fn render_error_view(msg: String) -> View {
    let i18n = use_context::<I18n>();
    div()
        .class("min-h-screen bg-gray-50 dark:bg-gray-900 flex items-center justify-center")
        .children(
            div()
                .class(
                    "bg-red-50 dark:bg-red-900/30 text-red-600 dark:text-red-400 px-6 py-4 rounded-lg",
                )
                .children((
                    p().class("font-semibold").children(i18n.t(K::LoadFailed)),
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

#[derive(Props)]
pub struct LayoutProps {
    pub dark: Signal<bool>,
    pub authenticated: Signal<Option<bool>>,
    pub username: Signal<Option<String>>,
}

#[component]
pub fn Layout(props: LayoutProps) -> View {
    let dark = props.dark;
    let authenticated = props.authenticated;
    let username = props.username;
    let sidebar_open = create_signal(false);

    let route = use_context::<ReadSignal<AppRoute>>();
    let i18n = use_context::<I18n>();

    // Auth guard: redirect to Login if not authenticated
    create_effect(move || {
        if authenticated.get() == Some(false) && route.get().is_console() {
            navigate("/login");
        }
    });

    // Reverse auth guard: redirect authenticated users away from login/register
    create_effect(move || {
        if authenticated.get() == Some(true)
            && (route.get() == AppRoute::Login || route.get() == AppRoute::Register)
        {
            navigate("/console/dashboard");
        }
    });

    // NotFound redirect
    create_effect(move || {
        if route.get() == AppRoute::NotFound {
            navigate("/");
        }
    });

    // Update page title based on route
    create_effect(move || {
        let route = route.get();
        let key = match route {
            AppRoute::Index => K::IndexTitle,
            AppRoute::Login => K::Login,
            AppRoute::Register => K::Register,
            AppRoute::Dashboard => K::Dashboard,
            AppRoute::Providers => K::Providers,
            AppRoute::Models => K::Models,
            AppRoute::ApiKeys => K::ApiKey,
            AppRoute::TextGeneration => K::TextGeneration,
            AppRoute::NotFound => K::Login,
        };
        let title = format!("Ait - {}", i18n.t(key));
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            doc.set_title(&title);
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
            username.get_clone(),
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
                AppRoute::Dashboard => {
                    let now = js_sys::Date::new_0();
                    let end_ts = (now.get_time() / 1000.0) as i64;
                    let today_midnight = js_sys::Date::new_with_year_month_day(
                        now.get_full_year(),
                        now.get_month() as i32,
                        now.get_date() as i32,
                    );
                    let start_ts = (today_midnight.get_time() / 1000.0) as i64 - 7 * 86400;
                    let (stats, req, tok) = futures_util::join!(
                        fetch_dashboard_stats(),
                        fetch_request_buckets(start_ts, end_ts),
                        fetch_token_buckets(start_ts, end_ts),
                    );
                    match (stats, req, tok) {
                        (Ok(s), Ok(r), Ok(t)) => RouteData::Dashboard(DashboardData {
                            provider_count: s.provider_count,
                            model_count: s.model_count,
                            api_request_count: s.api_request_count,
                            token_consumption: s.token_consumption,
                            request_buckets: r,
                            token_buckets: t,
                        }),
                        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                            RouteData::Error(e.to_string())
                        }
                    }
                }
                AppRoute::Providers => fetch_providers()
                    .await
                    .map(RouteData::Providers)
                    .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                AppRoute::Models => {
                    let (models, providers) =
                        futures_util::future::join(fetch_models(), fetch_providers()).await;
                    match (models, providers) {
                        (Ok(m), Ok(p)) => RouteData::Models(m, p),
                        (Err(e), _) | (_, Err(e)) => RouteData::Error(e.to_string()),
                    }
                }
                AppRoute::ApiKeys => match uname.get_clone() {
                    Some(u) => fetch_api_keys(&u)
                        .await
                        .map(RouteData::ApiKeys)
                        .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                    None => RouteData::Placeholder,
                },
                AppRoute::TextGeneration => fetch_models()
                    .await
                    .map(RouteData::TextGeneration)
                    .unwrap_or_else(|e| RouteData::Error(e.to_string())),
                _ => RouteData::Placeholder,
            };
            match route.get() {
                AppRoute::Providers => pr.set(false),
                AppRoute::Models => mr.set(false),
                AppRoute::ApiKeys => ar.set(false),
                _ => {}
            }
            result
        }
    }));

    div()
        .id("app")
        .class(move || if dark.get() { "dark" } else { "" })
        .children((
            View::from_dynamic(move || {
                if authenticated.get().is_none() {
                    render_loading()
                } else {
                    div()
                        .class("min-h-screen bg-gray-50 dark:bg-gray-900")
                        .children((
                            Topbar(TopbarProps { dark, authenticated, username }),
                            // Mobile overlay backdrop + sidebar
                            View::from_dynamic(move || {
                                if route.get().is_console() {
                                    div().children((
                                        div()
                                            .class(move || {
                                                if sidebar_open.get() {
                                                    "fixed inset-0 bg-black/50 z-30 md:hidden transition-opacity duration-300"
                                                } else {
                                                    "fixed inset-0 bg-black/50 z-30 md:hidden opacity-0 pointer-events-none transition-opacity duration-300"
                                                }
                                            })
                                            .on(events::click, move |_| sidebar_open.set(false)),
                                        div().children((
                                            Sidebar(SidebarProps { open: sidebar_open }),
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
                                        )),
                                    )).into()
                                } else {
                                    View::new()
                                }
                            }),
                            div()
                                .class(move || {
                                    if route.get().is_console() {
                                        "md:ml-60 min-h-[calc(100vh-3.5rem)] animate-fadeIn"
                                    } else {
                                        "min-h-[calc(100vh-3.5rem)] animate-fadeIn"
                                    }
                                })
                                .children(View::from_dynamic({
                                    let d = data.clone();
                                    move || {
                                    match route.get() {
                                        AppRoute::Index => {
                                            crate::views::index::render_index_view(
                                                authenticated,
                                            )
                                        }
                                        AppRoute::Login => {
                                            crate::views::login::render_login_view(
                                                authenticated,
                                                username,
                                            )
                                        }
                                        AppRoute::Register => {
                                            crate::views::register::render_register_view()
                                        }
                                        AppRoute::NotFound => render_loading(),
                                        _ => {
                                            let current_route = route.get();
                                            match d.get_clone() {
                                                Some(RouteData::Dashboard(d)) if current_route == AppRoute::Dashboard => {
                                                    crate::views::dashboard::render_dashboard_view(d)
                                                }
                                                Some(RouteData::Providers(p)) if current_route == AppRoute::Providers => {
                                                    crate::views::providers::render_providers_view(p, provider_refresh, provider_refreshing)
                                                }
                                                Some(RouteData::Models(m, p)) if current_route == AppRoute::Models => {
                                                    crate::views::models::render_models_view(m, p, model_refresh, model_refreshing)
                                                }
                                                Some(RouteData::ApiKeys(k)) if current_route == AppRoute::ApiKeys => {
                                                    let uname = username.get_clone().unwrap_or_default();
                                                    crate::views::api_keys::render_api_keys_view(k, uname, api_key_refresh, api_key_refreshing)
                                                }
                                                Some(RouteData::TextGeneration(m)) if current_route == AppRoute::TextGeneration => {
                                                    crate::views::text_generation::render_text_generation_view(m)
                                                }
                                                Some(RouteData::Error(e)) => render_error_view(e),
                                                _ => match current_route {
                                                    AppRoute::Dashboard => skeleton::dashboard_skeleton(),
                                                    AppRoute::Providers | AppRoute::Models | AppRoute::ApiKeys => skeleton::table_skeleton(),
                                                    AppRoute::TextGeneration => skeleton::text_gen_skeleton(),
                                                    _ => render_loading(),
                                                },
                                            }
                                        },
                                    }
                                }})),
                        ))
                        .into()
                }
            }),
            render_toast_container(),
        ))
        .into()
}
