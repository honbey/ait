use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::model_table::{ModelTable, ModelTableProps};
use crate::models::{Model, Provider};

pub fn render_models_view(
    models: Vec<Model>,
    providers: Vec<Provider>,
    is_admin: sycamore::reactive::Signal<bool>,
    model_refresh: sycamore::reactive::Signal<usize>,
    model_refreshing: sycamore::reactive::Signal<bool>,
) -> View {
    div()
        .class("p-4 sm:p-8")
        .children(ModelTable(ModelTableProps {
            models,
            providers,
            is_admin,
            model_refresh,
            model_refreshing,
        }))
        .into()
}
