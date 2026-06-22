use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

use crate::i18n::{I18n, K};
use crate::route::Route;

pub fn render_index_view(route: Signal<Route>, authenticated: Signal<bool>) -> View {
    let i18n = use_context::<I18n>();
    div()
        .class("min-h-[calc(100vh-3.5rem)] flex items-center justify-center bg-gray-50 dark:bg-gray-900")
        .children(
            div()
                .class("text-center px-4")
                .children((
                    h1()
                        .class("text-4xl sm:text-5xl lg:text-6xl font-bold text-gray-900 dark:text-gray-100 mb-6")
                        .children(i18n.t(K::IndexTitle)),
                    p()
                        .class("text-lg sm:text-xl text-gray-500 dark:text-gray-400 max-w-2xl mx-auto")
                        .children(i18n.t(K::IndexSubtitle)),
                    {
                        let i18n = i18n.clone();
                        View::from_dynamic(move || -> View {
                            if authenticated.get() {
                                div().class("mt-8 flex items-center justify-center gap-4").children(
                                    button()
                                        .class("px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg cursor-pointer transition-colors")
                                        .on(events::click, move |_| route.set(Route::Dashboard))
                                        .children(i18n.t(K::Console)),
                                )
                                    .into()
                            } else {
                                div().class("mt-8 flex items-center justify-center gap-4").children((
                                    button()
                                        .class("px-6 py-3 bg-indigo-600 hover:bg-indigo-700 text-white font-semibold rounded-lg cursor-pointer transition-colors")
                                        .on(events::click, move |_| route.set(Route::Login))
                                        .children(i18n.t(K::Login)),
                                    button()
                                        .class("px-6 py-3 border border-indigo-600 dark:border-indigo-400 text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 font-semibold rounded-lg cursor-pointer transition-colors")
                                        .on(events::click, move |_| route.set(Route::Register))
                                        .children(i18n.t(K::Register)),
                                ))
                                    .into()
                            }
                        })
                    },
                ))
        )
        .into()
}
