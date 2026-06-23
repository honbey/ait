use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::stat_card::StatCard;
use crate::components::stat_card::StatCardProps;
use crate::i18n::{I18n, K};
use crate::models::DashboardData;

pub fn render_dashboard_view(data: DashboardData) -> View {
    let i18n = use_context::<I18n>();
    let provider_count = data.provider_count;
    let model_count = data.model_count;
    let api_calls = data.api_request_count;
    let token_consumption = data.token_consumption;

    div().children(
            div()
                .class("p-4 sm:p-8 space-y-6 sm:space-y-8")
                .children((
                    div()
                        .class("grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-6")
                        .children((
                            StatCard(StatCardProps {
                                icon: "fa-server",
                                bg_color: "bg-indigo-100 text-indigo-600 dark:bg-indigo-900 dark:text-indigo-400",
                                value: provider_count.to_string(),
                                label: i18n.t(K::Providers),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-cube",
                                bg_color: "bg-green-100 text-green-600 dark:bg-green-900 dark:text-green-400",
                                value: model_count.to_string(),
                                label: i18n.t(K::Models),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-arrow-right-arrow-left",
                                bg_color: "bg-amber-100 text-amber-600 dark:bg-amber-900 dark:text-amber-400",
                                value: api_calls.to_string(),
                                label: i18n.t(K::ApiRequestCount),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-code",
                                bg_color: "bg-pink-100 text-pink-600 dark:bg-pink-900 dark:text-pink-400",
                                value: token_consumption.to_string(),
                                label: i18n.t(K::TokenConsumption),
                            }),
                        )),
                    div()
                        .class("grid grid-cols-1 lg:grid-cols-2 gap-6 mt-8")
                        .children((
                            div()
                                .class(
                                    "bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm",
                                )
                                .children((
                                    h2().class(
                                        "text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4",
                                    )
                                    .children(i18n.t(K::ApiRequestCountTrendingTableTitle)),
                                    div()
                                        .class(
                                            "h-64 flex items-center justify-center bg-gray-50 dark:bg-gray-700 rounded-lg border-2 border-dashed border-gray-300 dark:border-gray-600",
                                        )
                                        .children(
                                            span().class("text-gray-400 text-sm")
                                                .children(i18n.t(K::ChartPlaceholder)),
                                        ),
                                )),
                            div()
                                .class(
                                    "bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm",
                                )
                                .children((
                                    h2().class(
                                        "text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4",
                                    )
                                    .children(i18n.t(K::TokenConsumptionTrendingTableTitle)),
                                    div()
                                        .class(
                                            "h-64 flex items-center justify-center bg-gray-50 dark:bg-gray-700 rounded-lg border-2 border-dashed border-gray-300 dark:border-gray-600",
                                        )
                                        .children(
                                            span().class("text-gray-400 text-sm")
                                                .children(i18n.t(K::ChartPlaceholder)),
                                        ),
                                )),
                        )),
                )),
        )
        .into()
}
