use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;
use sycamore_router::navigate;

use super::dark_mode_toggle::DarkModeToggle;
use super::dark_mode_toggle::DarkModeToggleProps;
use crate::i18n::{I18n, K};
use crate::route::AppRoute;

fn nav_item(
    href: &'static str,
    is_active: impl Fn(AppRoute) -> bool + 'static,
    icon: &str,
    i18n_key: K,
    mobile_hidden: bool,
) -> View {
    let i18n = use_context::<I18n>();
    let route = use_context::<ReadSignal<AppRoute>>();
    let (active, inactive) = if mobile_hidden {
        (
            "hidden md:flex items-center gap-2 px-3 py-2 text-indigo-600 dark:text-indigo-400 font-semibold text-sm",
            "hidden md:flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm",
        )
    } else {
        (
            "flex items-center gap-2 px-3 py-2 text-indigo-600 dark:text-indigo-400 font-semibold text-sm",
            "flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm",
        )
    };
    let icon_class = format!("{icon} w-4 text-center");
    a()
        .attr("href", href)
        .class(move || {
            if is_active(route.get()) {
                active
            } else {
                inactive
            }
        })
        .children((
            i().class(icon_class),
            span().class("hidden sm:inline").children({
                let i18n = i18n.clone();
                View::from_dynamic(move || i18n.t(i18n_key))
            }),
        ))
        .into()
}

fn nav_item_static(icon: &str, i18n_key: K) -> View {
    let i18n = use_context::<I18n>();
    let icon_class = format!("{icon} w-4 text-center");
    a()
        .attr("href", "/")
        .class("hidden md:flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm")
        .children((
            i().class(icon_class),
            span().class("hidden sm:inline").children({
                let i18n = i18n.clone();
                View::from_dynamic(move || i18n.t(i18n_key))
            }),
        ))
        .into()
}

#[derive(Props)]
pub struct TopbarProps {
    pub dark: Signal<bool>,
    pub authenticated: Signal<bool>,
    pub username: Signal<Option<String>>,
}

#[component]
pub fn Topbar(props: TopbarProps) -> View {
    let i18n = use_context::<I18n>();
    let authenticated = props.authenticated;
    let username = props.username;

    nav()
        .class(
            "bg-white dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700 px-4 sm:px-6 flex items-center justify-between h-14 sticky top-0 z-50",
        )
        .children((
            div().class("flex items-center gap-4").children((
                a()
                    .attr("href", "/")
                    .class("flex items-center gap-2 mr-2 cursor-pointer")
                    .children((
                        img().class("h-8").src("ait-logo.svg").alt("ait"),
                        span().class("text-xl font-bold text-gray-900 dark:text-gray-100")
                            .children("Ait"),
                    )),
                div().class("flex items-center gap-1").children((
                    nav_item("/", |r| r == AppRoute::Index, "fas fa-house", K::Index, true),
                    nav_item("/console/dashboard", |r| r.is_console(), "fas fa-terminal", K::Console, false),
                    nav_item_static("fas fa-book", K::Docs),
                    nav_item_static("fas fa-info-circle", K::About),
                )),
            )),
            div().class("flex items-center gap-1 sm:gap-3").children((
                {
                    let i18n_lang = i18n.clone();
                    let i18n_label = i18n.clone();
                    button()
                        .class(
                            "hidden sm:flex items-center gap-1.5 px-2 py-2 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm",
                        )
                        .on(events::click, move |_| {
                            let current = i18n_lang.lang();
                            let new_lang = if current == "zh" { "en" } else { "zh" };
                            i18n_lang.set_lang(new_lang);
                        })
                        .children((
                            i().class("fas fa-globe w-4 text-center"),
                            span().class("hidden sm:inline").children(View::from_dynamic(move || i18n_label.t(K::Language))),
                        ))
                },
                DarkModeToggle(DarkModeToggleProps { dark: props.dark }),
                div().class("hidden sm:block w-px h-6 bg-gray-200 dark:bg-gray-700 mx-1"),
                {
                    let show_dropdown = create_signal(false);

                    View::from_dynamic(move || -> sycamore::web::View {
                        if !authenticated.get() {
                            let i18n_login = i18n.clone();
                            a()
                                .attr("href", "/login")
                                .class(
                                    "flex items-center gap-2 px-3 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer transition-colors text-sm text-gray-600 dark:text-gray-300",
                                )
                                .children((
                                    i().class("fas fa-sign-in-alt w-4 text-center"),
                                    span().class("hidden sm:inline")
                                        .children(View::from_dynamic(move || i18n_login.t(K::Login))),
                                ))
                                .into()
                        } else {
                            let i18n_logout = i18n.clone();
                            let uname = username;
                            let avatar_letter = create_memo(move || {
                                uname.get_clone()
                                .and_then(|s| s.chars().next().map(|c| c.to_uppercase().to_string()))
                                .unwrap_or_else(|| "U".to_string())
                            });
                            div()
                                .class("relative")
                                .children((
                                    div()
                                        .class(
                                            "flex items-center gap-2 px-2 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer transition-colors relative z-50",
                                        )
                                        .on(events::click, move |_| show_dropdown.set(!show_dropdown.get()))
                                        .children((
                                            div()
                                                .class(
                                                    "w-7 h-7 rounded-full bg-indigo-100 dark:bg-indigo-900 flex items-center justify-center text-indigo-600 dark:text-indigo-400 text-xs font-semibold",
                                                )
                                                .children(avatar_letter),
                                            span().class("hidden sm:inline text-sm text-gray-700 dark:text-gray-300")
                                                .children(View::from_dynamic({
                                                    move || uname.get_clone().unwrap_or_default()
                                                })),
                                            i().class(
                                                "fas fa-chevron-down text-[10px] text-gray-400 dark:text-gray-500 hidden sm:inline",
                                            ),
                                        )),
                                    // Backdrop for outside-click-to-close
                                    View::from_dynamic(move || {
                                        if show_dropdown.get() {
                                            div()
                                                .class("fixed inset-0 z-40")
                                                .on(events::click, move |_| show_dropdown.set(false))
                                                .into()
                                        } else {
                                            View::new()
                                        }
                                    }),
                                    // Dropdown menu
                                    View::from_dynamic(move || {
                                        if show_dropdown.get() {
                                            div()
                                                .class("absolute right-0 top-full mt-2 w-32 bg-white dark:bg-gray-800 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700 py-1 z-50")
                                                .on(events::click, move |e: web_sys::MouseEvent| e.stop_propagation())
                                                .children(
                                                    button()
                                                        .class("w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 cursor-pointer")
                                                        .on(events::click, move |_e: web_sys::MouseEvent| {
                                                            spawn_local_scoped(async move {
                                                                crate::api::logout_api().await.ok();
                                                                Timeout::new(0, move || {
                                                                    show_dropdown.set(false);
                                                                    authenticated.set(false);
                                                                    navigate("/");
                                                                }).forget();
                                                            });
                                                        })
                                                        .children(i18n_logout.t(K::Logout)),
                                                )
                                                .into()
                                        } else {
                                            View::new()
                                        }
                                    }),
                                ))
                                .into()
                        }
                    })
                },
            )),
        ))
        .into()
}
