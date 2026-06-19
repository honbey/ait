use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::api_key_table::{ApiKeyTable, ApiKeyTableProps};
use crate::models::ApiKeyListItem;

pub fn render_api_keys_view(
    keys: Vec<ApiKeyListItem>,
    username: String,
    api_key_refresh: sycamore::reactive::Signal<usize>,
    api_key_refreshing: sycamore::reactive::Signal<bool>,
) -> View {
    div().class("p-4 sm:p-8").children(
        ApiKeyTable(ApiKeyTableProps { keys, username, api_key_refresh, api_key_refreshing }),
    )
    .into()
}
