use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::provider_table::ProviderTable;
use crate::components::provider_table::ProviderTableProps;
use crate::models::Provider;

pub fn render_providers_view(
    providers: Vec<Provider>,
    is_admin: sycamore::reactive::Signal<bool>,
    provider_refresh: sycamore::reactive::Signal<usize>,
    provider_refreshing: sycamore::reactive::Signal<bool>,
) -> View {
    div()
        .children(
            div()
                .class("p-4 sm:p-8")
                .children(ProviderTable(ProviderTableProps {
                    providers,
                    is_admin,
                    provider_refresh,
                    provider_refreshing,
                })),
        )
        .into()
}
