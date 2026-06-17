use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::i18n::I18n;

pub fn render_index_view(i18n: &I18n) -> View {
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
                ))
        )
        .into()
}
