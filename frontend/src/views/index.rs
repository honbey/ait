use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::i18n::I18n;
use crate::route::Route;

pub fn render_index_view(i18n: &I18n, route: Signal<Route>) -> View {
    div()
        .class("min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900")
        .children(
            div()
                .class("text-center px-4")
                .children((
                    h1()
                        .class("text-4xl sm:text-5xl lg:text-6xl font-bold text-gray-900 dark:text-gray-100 mb-6")
                        .children(i18n.t("index_title")),
                    p()
                        .class("text-lg sm:text-xl text-gray-500 dark:text-gray-400 max-w-2xl mx-auto")
                        .children(i18n.t("index_subtitle")),
                    div().class("mt-8 flex items-center justify-center gap-4").children((
                        button()
                            .class("px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg transition-colors")
                            .on(events::click, move |_| route.set(Route::Login))
                            .children(i18n.t("login")),
                        button()
                            .class("px-6 py-3 border border-indigo-600 dark:border-indigo-400 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 font-semibold rounded-lg transition-colors")
                            .on(events::click, move |_| route.set(Route::Register))
                            .children(i18n.t("register")),
                    )),
                ))
        )
        .into()
}
