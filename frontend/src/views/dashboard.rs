use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::chart::ChartSeries;
use crate::components::line_chart::{LineChart, LineChartProps};
use crate::components::modal::{CLASS_CARD, CLASS_PAGE_SHELL};
use crate::components::stat_card::StatCard;
use crate::components::stat_card::StatCardProps;
use crate::i18n::{I18n, K};
use crate::models::{BucketEntry, DashboardData, format_timestamp};

fn aggregate_daily(buckets: &[BucketEntry]) -> Vec<BucketEntry> {
    let mut daily: Vec<BucketEntry> = Vec::new();
    for b in buckets {
        let d = js_sys::Date::new(&(b.timestamp * 1000.0).into());
        let day = js_sys::Date::new_with_year_month_day(
            d.get_full_year(),
            d.get_month() as i32,
            d.get_date() as i32,
        );
        let day_ts = day.get_time() / 1000.0;
        if let Some(last) = daily.last_mut()
            && (last.timestamp - day_ts).abs() < 1.0
        {
            last.count += b.count;
        } else {
            daily.push(BucketEntry {
                timestamp: day_ts,
                count: b.count,
            });
        }
    }
    daily
}

fn fill_daily_range(daily: &[BucketEntry], start_ts: f64, end_ts: f64) -> Vec<BucketEntry> {
    let day_secs = 86400.0;
    let sd = js_sys::Date::new(&(start_ts * 1000.0).into());
    let ed = js_sys::Date::new(&(end_ts * 1000.0).into());
    let start_day = js_sys::Date::new_with_year_month_day(
        sd.get_full_year(),
        sd.get_month() as i32,
        sd.get_date() as i32,
    );
    let end_day = js_sys::Date::new_with_year_month_day(
        ed.get_full_year(),
        ed.get_month() as i32,
        ed.get_date() as i32,
    );
    let start_day_ts = start_day.get_time() / 1000.0;
    let end_day_ts = end_day.get_time() / 1000.0;

    let mut result = Vec::new();
    let mut i = 0;
    let mut cur = start_day_ts;
    while cur <= end_day_ts {
        if i < daily.len() && (daily[i].timestamp - cur).abs() < 1.0 {
            result.push(daily[i].clone());
            i += 1;
        } else {
            result.push(BucketEntry {
                timestamp: cur,
                count: 0,
            });
        }
        cur += day_secs;
    }
    result
}

pub fn render_dashboard_view(data: DashboardData) -> View {
    let i18n = use_context::<I18n>();

    let now = js_sys::Date::new_0();
    let end_ts = now.get_time() / 1000.0;
    let today_midnight = js_sys::Date::new_with_year_month_day(
        now.get_full_year(),
        now.get_month() as i32,
        now.get_date() as i32,
    );
    let start_ts = (today_midnight.get_time() / 1000.0) - 7.0 * 86400.0;

    let req = fill_daily_range(&aggregate_daily(&data.request_buckets), start_ts, end_ts);
    let tok = fill_daily_range(&aggregate_daily(&data.token_buckets), start_ts, end_ts);

    let req_x_data: Vec<String> = req.iter().map(|r| format_timestamp(r.timestamp)).collect();
    let req_series = vec![ChartSeries {
        name: "Requests".to_string(),
        data: req.iter().map(|r| r.count as f64).collect(),
    }];

    let tok_x_data: Vec<String> = tok.iter().map(|r| format_timestamp(r.timestamp)).collect();
    let tok_series = vec![ChartSeries {
        name: "Tokens".to_string(),
        data: tok.iter().map(|r| r.count as f64).collect(),
    }];

    div().children(
            div()
                .class(format!("{} space-y-6 sm:space-y-8", CLASS_PAGE_SHELL))
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
                                .class(format!("{} p-6", CLASS_CARD))
                                .children((
                                    h3().class("text-lg font-semibold text-gray-800 dark:text-gray-200 mb-4")
                                        .children(i18n.t(K::ApiRequestCountTrendingTableTitle)),
                                    LineChart(LineChartProps {
                                        id: "chart-requests",
                                        x_data: req_x_data,
                                        series_list: req_series,
                                    }),
                                )),
                            div()
                                .class(format!("{} p-6", CLASS_CARD))
                                .children((
                                    h3().class("text-lg font-semibold text-gray-800 dark:text-gray-200 mb-4")
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
