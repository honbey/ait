use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::modal::CLASS_PAGE_SHELL;
use crate::components::provider_table::ProviderTable;
use crate::components::provider_table::ProviderTableProps;
use crate::models::Provider;

pub fn render_providers_view(
    providers: Vec<Provider>,
    provider_refresh: sycamore::reactive::Signal<usize>,
    provider_refreshing: sycamore::reactive::Signal<bool>,
) -> View {
    div()
        .class(CLASS_PAGE_SHELL)
        .children(ProviderTable(ProviderTableProps {
            providers,
            provider_refresh,
            provider_refreshing,
        }))
        .into()
}
