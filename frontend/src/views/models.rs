use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::model_table::ModelTable;
use crate::components::model_table::ModelTableProps;
use crate::i18n::I18n;
use crate::models::Model;

pub fn render_models_view(i18n: &I18n, models: Vec<Model>) -> View {
    div().children(
        div()
            .class("p-4 sm:p-8 space-y-6 sm:space-y-8")
            .children((
                div()
                    .class("flex items-center justify-between")
                    .children((
                        h1().class("text-2xl font-bold text-gray-800 dark:text-gray-100")
                            .children(i18n.t("model_management")),
                        span().class(
                            "text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-3 py-1 rounded-full",
                        )
                        .children(i18n.t_replace("total_count", "count", &models.len().to_string())),
                    )),
                ModelTable(ModelTableProps { models }),
            )),
    )
    .into()
}
