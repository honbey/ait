use sycamore::prelude::*;
use sycamore::web::bind;
use sycamore::web::events;
use sycamore::web::tags::*;
use sycamore_futures::spawn_local_scoped;

use crate::i18n::I18n;
use crate::route::Route;

pub fn render_login_view(i18n: &I18n, authenticated: Signal<bool>, route: Signal<Route>) -> View {
    let username = create_signal(String::new());
    let password = create_signal(String::new());
    let error = create_signal(String::new());
    let loading = create_signal(false);

    let i18n_submit = i18n.clone();
    let on_submit = move |ev: web_sys::SubmitEvent| {
        ev.prevent_default();

        if loading.get() {
            return;
        }

        if username.get_clone().is_empty() || password.get_clone().is_empty() {
            error.set(i18n_submit.t("login_required"));
            return;
        }

        loading.set(true);
        let u = username.get_clone();
        let p = password.get_clone();
        let i18n_async = i18n_submit.clone();
        let loading_async = loading;
        spawn_local_scoped(async move {
            match crate::api::login_api(&u, &p).await {
                Ok(()) => {
                    authenticated.set(true);
                    route.set(Route::Dashboard);
                }
                Err(e) => {
                    error.set(i18n_async.t_replace("login_error", "msg", &e));
                    loading_async.set(false);
                }
            }
        });
    };

    form()
        .on(events::submit, on_submit)
        .class("min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900")
        .children(
            div()
                .class("bg-white dark:bg-gray-800 rounded-xl shadow-sm p-8 w-full max-w-md mx-4")
                .children((
                    h2()
                        .class("text-2xl font-bold text-gray-900 dark:text-gray-100 mb-6 text-center")
                        .children(i18n.t("login")),
                    div().class("mb-4").children((
                        label()
                            .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                            .children(i18n.t("username")),
                        input()
                            .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                            .attr("type", "text")
                            .attr("placeholder", i18n.t("username"))
                            .bind(bind::value, username)
                            .on(events::input, move |_| error.set(String::new())),
                    )),
                    div().class("mb-6").children((
                        label()
                            .class("block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1")
                            .children(i18n.t("password")),
                        input()
                            .class("w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:ring-2 focus:ring-indigo-500 focus:border-indigo-500 outline-none")
                            .attr("type", "password")
                            .attr("placeholder", i18n.t("password"))
                            .bind(bind::value, password)
                            .on(events::input, move |_| error.set(String::new())),
                    )),
                    View::from_dynamic(move || {
                        let err = error.get_clone();
                        if err.is_empty() {
                            View::new()
                        } else {
                            p().class("text-red-500 text-sm mb-4").children(err).into()
                        }
                    }),
                    {
                        let i18n_btn = i18n.clone();
                        button()
                            .attr("type", "submit")
                            .disabled(move || loading.get())
                            .class("w-full py-2 px-4 bg-indigo-600 hover:enabled:bg-indigo-700 text-white font-semibold rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2")
                            .children(View::from_dynamic(move || -> View {
                                if loading.get() {
                                    div().class("flex items-center gap-2").children((
                                        i().class("fas fa-spinner animate-spin"),
                                        span().children(i18n_btn.t("login_btn")),
                                    )).into()
                                } else {
                                    span().children(i18n_btn.t("login_btn")).into()
                                }
                            }))
                    },
                )),
        )
        .into()
}
