use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::chart::ChartSeries;
use crate::components::line_chart::{LineChart, LineChartProps};
use crate::components::stat_card::StatCard;
use crate::components::stat_card::StatCardProps;
use crate::i18n::{I18n, K};
use crate::models::DashboardData;

pub fn render_dashboard_view(data: DashboardData) -> View {
    let i18n = use_context::<I18n>();

    let req_x_data: Vec<String> = data
        .daily_requests
        .iter()
        .map(|r| r.date.clone())
        .collect();
    let req_series = vec![ChartSeries {
        name: "Requests".to_string(),
        data: data.daily_requests.iter().map(|r| r.count as f64).collect(),
    }];

    let tok_x_data: Vec<String> = data
        .daily_tokens
        .iter()
        .map(|r| r.date.clone())
        .collect();
    let tok_series = vec![ChartSeries {
        name: "Tokens".to_string(),
        data: data.daily_tokens.iter().map(|r| r.tokens as f64).collect(),
    }];

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
                                value: data.provider_count.to_string(),
                                label: i18n.t(K::Providers),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-cube",
                                bg_color: "bg-green-100 text-green-600 dark:bg-green-900 dark:text-green-400",
                                value: data.model_count.to_string(),
                                label: i18n.t(K::Models),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-arrow-right-arrow-left",
                                bg_color: "bg-amber-100 text-amber-600 dark:bg-amber-900 dark:text-amber-400",
                                value: data.api_request_count.to_string(),
                                label: i18n.t(K::ApiRequestCount),
                            }),
                            StatCard(StatCardProps {
                                icon: "fa-code",
                                bg_color: "bg-pink-100 text-pink-600 dark:bg-pink-900 dark:text-pink-400",
                                value: data.token_consumption.to_string(),
                                label: i18n.t(K::TokenConsumption),
                            }),
                        )),
                    div()
                        .class("grid grid-cols-1 lg:grid-cols-2 gap-6 mt-8")
                        .children((
                            div()
                                .class("bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm")
                                .children((
                                    h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4")
                                        .children(i18n.t(K::ApiRequestCountTrendingTableTitle)),
                                    LineChart(LineChartProps {
                                        id: "chart-requests",
                                        x_data: req_x_data,
                                        series_list: req_series,
                                    }),
                                )),
                            div()
                                .class("bg-white dark:bg-gray-800 rounded-xl p-6 shadow-sm")
                                .children((
                                    h2().class("text-lg font-semibold text-gray-800 dark:text-gray-100 mb-4")
                                        .children(i18n.t(K::TokenConsumptionTrendingTableTitle)),
                                    LineChart(LineChartProps {
                                        id: "chart-tokens",
                                        x_data: tok_x_data,
                                        series_list: tok_series,
                                    }),
                                )),
                        )),
                )),
        )
        .into()
}
