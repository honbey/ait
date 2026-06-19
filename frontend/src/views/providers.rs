use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::provider_table::ProviderTable;
use crate::components::provider_table::ProviderTableProps;
use crate::i18n::I18n;
use crate::models::Provider;

pub fn render_providers_view(i18n: &I18n, providers: Vec<Provider>) -> View {
    div().children(
        div()
            .class("p-4 sm:p-8 space-y-6 sm:space-y-8")
            .children((
                div()
                    .class("flex items-center justify-between")
                    .children((
                        h1().class("text-2xl font-bold text-gray-800 dark:text-gray-100")
                            .children(i18n.t("providers_management")),
                        span().class(
                            "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                        )
                        .children(i18n.t_replace("total_count", "count", &providers.len().to_string())),
                    )),
                ProviderTable(ProviderTableProps { providers }),
            )),
    )
    .into()
}
