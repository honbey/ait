use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use super::dark_mode_toggle::DarkModeToggle;
use super::dark_mode_toggle::DarkModeToggleProps;
use crate::i18n::I18n;
use crate::route::Route;

#[derive(Props)]
pub struct TopbarProps {
    pub dark: Signal<bool>,
    pub route: Signal<Route>,
    pub authenticated: Signal<bool>,
    pub username: Signal<Option<String>>,
}

#[component]
pub fn Topbar(props: TopbarProps) -> View {
    let i18n = use_context::<I18n>();
    let route = props.route;
    let authenticated = props.authenticated;
    let username = props.username;

    nav()
        .class(
            "bg-white dark:bg-gray-900 border-b border-gray-200 dark:border-gray-700 px-4 sm:px-6 flex items-center justify-between h-14 sticky top-0 z-50",
        )
        .children((
            div().class("flex items-center gap-4").children((
                div()
                    .class("flex items-center gap-2 cursor-pointer mr-2")
                    .on(events::click, move |_| route.set(Route::Index))
                    .children((
                        img().class("h-8").src("ait-logo.svg").alt("ait"),
                        span().class("text-xl font-bold text-gray-900 dark:text-gray-100")
                            .children("Ait"),
                    )),
                div().class("flex items-center gap-1").children((
                    // Index
                    div()
                        .class(move || {
                            if route.get() == Route::Index {
                                "hidden md:flex items-center gap-2 px-3 py-2 text-indigo-600 dark:text-indigo-400 font-semibold text-sm"
                            } else {
                                "hidden md:flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm"
                            }
                        })
                        .on(events::click, move |_| route.set(Route::Index))
                        .children((
                            i().class("fas fa-house w-4 text-center"),
                            span().class("hidden sm:inline").children({
                                let i18n = i18n.clone();
                                View::from_dynamic(move || i18n.t("index"))
                            }),
                        )),
                    // Console
                    div()
                        .class(move || {
                            if route.get().is_console() {
                                "flex items-center gap-2 px-3 py-2 text-indigo-600 dark:text-indigo-400 font-semibold text-sm"
                            } else {
                                "flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm"
                            }
                        })
                        .on(events::click, move |_| {
                            if authenticated.get() {
                                route.set(Route::Dashboard);
                            } else {
                                route.set(Route::Login);
                            }
                        })
                        .children((
                            i().class("fas fa-terminal w-4 text-center"),
                            span().class("hidden md:inline").children({
                                let i18n = i18n.clone();
                                View::from_dynamic(move || i18n.t("console"))
                            }),
                        )),
                    // Docs
                    div()
                        .class("hidden md:flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm")
                        .children((
                            i().class("fas fa-book w-4 text-center"),
                            span().class("hidden sm:inline").children({
                                let i18n = i18n.clone();
                                View::from_dynamic(move || i18n.t("docs"))
                            }),
                        )),
                    // About
                    div()
                        .class("hidden md:flex items-center gap-2 px-3 py-2 text-gray-600 dark:text-gray-300 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors text-sm")
                        .children((
                            i().class("fas fa-info-circle w-4 text-center"),
                            span().class("hidden sm:inline").children({
                                let i18n = i18n.clone();
                                View::from_dynamic(move || i18n.t("about"))
                            }),
                        )),
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
                            span().class("hidden sm:inline").children(View::from_dynamic(move || i18n_label.t("language"))),
                        ))
                },
                DarkModeToggle(DarkModeToggleProps { dark: props.dark }),
                div().class("hidden sm:block w-px h-6 bg-gray-200 dark:bg-gray-700 mx-1"),
                {
                    let show_dropdown = create_signal(false);
                    let auth = authenticated;
                    let uname = username;
                    let r = route;

                    View::from_dynamic(move || -> sycamore::web::View {
                        if !auth.get() {
                            let i18n_login = i18n.clone();
                            button()
                                .class(
                                    "flex items-center gap-2 px-3 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer transition-colors text-sm text-gray-600 dark:text-gray-300",
                                )
                                .on(events::click, move |_| r.set(crate::route::Route::Login))
                                .children((
                                    i().class("fas fa-sign-in-alt w-4 text-center"),
                                    span().class("hidden sm:inline")
                                        .children(View::from_dynamic(move || i18n_login.t("login"))),
                                ))
                                .into()
                        } else {
                            let i18n_logout = i18n.clone();
                            let avatar_letter = uname.get_clone().and_then(|s| s.chars().next().map(|c| c.to_uppercase().to_string())).unwrap_or_else(|| "U".to_string());
                            div()
                                .class("relative")
                                .children((
                                    div()
                                        .class(
                                            "flex items-center gap-2 cursor-pointer px-2 py-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors relative z-50",
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
                                                        .class("w-full text-left px-4 py-2 text-sm text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700")
                                                        .on(events::click, move |_e: web_sys::MouseEvent| {
                                                            show_dropdown.set(false);
                                                            let a = auth;
                                                            let rt = r;
                                                            spawn_local_scoped(async move {
                                                                crate::api::logout_api().await.ok();
                                                                a.set(false);
                                                                rt.set(crate::route::Route::Index);
                                                            });
                                                        })
                                                        .children(i18n_logout.t("logout")),
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
