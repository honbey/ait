use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::modal::CLASS_PAGE_SHELL;
use crate::components::model_table::{ModelTable, ModelTableProps};
use crate::models::{Model, Provider};

pub fn render_models_view(
    models: Vec<Model>,
    providers: Vec<Provider>,
    model_refresh: sycamore::reactive::Signal<usize>,
    model_refreshing: sycamore::reactive::Signal<bool>,
) -> View {
    div()
        .class(CLASS_PAGE_SHELL)
        .children(ModelTable(ModelTableProps {
            models,
            providers,
            model_refresh,
            model_refreshing,
        }))
        .into()
}
