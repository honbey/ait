use leptos::prelude::*;

use crate::api::{self, BucketEntry};
use crate::auth::AuthContext;
use crate::components::error_display::ErrorCard;
use crate::components::line_chart::{ChartSeries, LineChart};
use crate::components::pie_chart::{PieChart, PieData};
use crate::components::skeleton::overview_skeleton;
use crate::components::style::{CLASS_CARD, CLASS_TEXT_MUTED};
use crate::components::use_page_title;
use crate::time_utils::{clamp_range, date_str_to_ts, midnight_ts, now_timestamp, ts_to_date_str};
use crate::{t, tr, ts};

fn aggregate_daily(buckets: &[BucketEntry]) -> Vec<BucketEntry> {
    let mut daily: Vec<BucketEntry> = Vec::new();
    for b in buckets {
        let d = js_sys::Date::new(&((b.timestamp as f64) * 1000.0).into());
        let day = js_sys::Date::new_with_year_month_day(
            d.get_full_year(),
            d.get_month() as i32,
            d.get_date() as i32,
        );
        let day_ts = (day.get_time() / 1000.0) as i64;
        if let Some(last) = daily.last_mut()
            && (last.timestamp - day_ts).abs() < 1
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

fn fill_daily_range(daily: &[BucketEntry], start_ts: i64, end_ts: i64) -> Vec<BucketEntry> {
    let day_secs = 86400i64;
    let sd = js_sys::Date::new(&((start_ts as f64) * 1000.0).into());
    let ed = js_sys::Date::new(&((end_ts as f64) * 1000.0).into());
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
    let start_day_ts = (start_day.get_time() / 1000.0) as i64;
    let end_day_ts = (end_day.get_time() / 1000.0) as i64;

    let mut result = Vec::new();
    let mut i = 0;
    let mut cur = start_day_ts;
    while cur <= end_day_ts {
        if i < daily.len() && (daily[i].timestamp - cur).abs() < 1 {
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

#[derive(Clone)]
struct OverviewData {
    provider_count: u64,
    model_count: u64,
    api_request_count: u64,
    token_consumption: u64,
    rpm: f64,
    tpm: f64,
    request_buckets: Vec<BucketEntry>,
    token_buckets: Vec<BucketEntry>,
    model_dist: Vec<api::ModelDistEntry>,
    token_dist: Vec<api::TokenDistEntry>,
}

#[component]
fn StatCard(
    icon: &'static str,
    icon_bg: &'static str,
    value: String,
    label: impl IntoView,
) -> impl IntoView {
    view! {
        <div class="bg-white dark:bg-gray-800 rounded-xl p-6 flex items-center gap-4 shadow-sm">
            <div class=format!(
                "w-14 h-14 rounded-full flex items-center justify-center text-xl {}",
                icon_bg,
            )>
                <i class=format!("fas {}", icon)></i>
            </div>
            <div>
                <div class="text-3xl font-bold text-gray-800 dark:text-gray-100">{value}</div>
                <div class=format!("text-sm {}", CLASS_TEXT_MUTED)>{label}</div>
            </div>
        </div>
    }
}

#[component]
fn TabButton(active: bool, on_click: impl Fn() + 'static, label: impl IntoView) -> impl IntoView {
    let cls = if active {
        "px-4 py-1.5 text-sm font-medium rounded-lg \
          bg-indigo-600 text-white shadow-sm cursor-pointer"
    } else {
        "px-4 py-1.5 text-sm font-medium rounded-lg \
          bg-gray-100 dark:bg-gray-700 \
          text-gray-600 dark:text-gray-300 \
          hover:bg-gray-200 dark:hover:bg-gray-600 cursor-pointer"
    };
    view! {
        <button class=cls on:click=move |_| on_click()>
            {label}
        </button>
    }
}

#[component]
pub fn Overview() -> impl IntoView {
    use_page_title(&format!("Ait - {}", ts!(Overview)));

    let auth = use_context::<AuthContext>().expect("AuthContext");

    let now = now_timestamp();
    let today = midnight_ts(now);
    let default_end = now;
    let default_start = today - 6 * 86400;

    let (start_ts, set_start_ts) = signal(default_start);
    let (end_ts, set_end_ts) = signal(default_end);
    let (left_tab, set_left_tab) = signal(0usize);
    let (right_tab, set_right_tab) = signal(0usize);

    let overview_resource: LocalResource<Result<OverviewData, String>> = LocalResource::new({
        move || {
            let s = start_ts.get_untracked();
            let e = end_ts.get_untracked();
            async move {
                let (stats, req_b, tok_b, mdl, tok_d) = futures_util::join!(
                    api::fetch_overview_stats(s, e),
                    api::fetch_request_buckets(s, e),
                    api::fetch_token_buckets(s, e),
                    api::fetch_model_dist(s, e),
                    api::fetch_token_dist(s, e),
                );
                match (stats, req_b, tok_b, mdl, tok_d) {
                    (Ok(s), Ok(r), Ok(t), Ok(md), Ok(td)) => Ok(OverviewData {
                        provider_count: s.provider_count,
                        model_count: s.model_count,
                        api_request_count: s.api_request_count,
                        token_consumption: s.token_consumption,
                        rpm: s.rpm,
                        tpm: s.tpm,
                        request_buckets: r,
                        token_buckets: t,
                        model_dist: md,
                        token_dist: td,
                    }),
                    (Err(e), _, _, _, _)
                    | (_, Err(e), _, _, _)
                    | (_, _, Err(e), _, _)
                    | (_, _, _, Err(e), _)
                    | (_, _, _, _, Err(e)) => Err(e.to_string()),
                }
            }
        }
    });

    let set_range = move |start: i64, end: i64| {
        let now = now_timestamp();
        let (start, end) = clamp_range(start, end, now);
        set_start_ts.set(start);
        set_end_ts.set(end);
        overview_resource.refetch();
    };

    let refresh = move || {
        let now = now_timestamp();
        let (start, end) = clamp_range(start_ts.get_untracked(), end_ts.get_untracked(), now);
        set_start_ts.set(start);
        set_end_ts.set(end);
        overview_resource.refetch();
    };

    let on_start_date = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        if let Some(ts) = date_str_to_ts(&val) {
            set_start_ts.set(ts);
        }
    };

    let on_end_date = move |ev: leptos::ev::Event| {
        let val = event_target_value(&ev);
        if let Some(ts) = date_str_to_ts(&val) {
            set_end_ts.set(ts);
        }
    };

    // Chart data signals (persistent across data refreshes)
    let req_x_data = RwSignal::new(Vec::<String>::new());
    let req_series = RwSignal::new(Vec::<ChartSeries>::new());
    let tok_x_data = RwSignal::new(Vec::<String>::new());
    let tok_series = RwSignal::new(Vec::<ChartSeries>::new());
    let model_pie_data = RwSignal::new(Vec::<PieData>::new());
    let token_pie_data = RwSignal::new(Vec::<PieData>::new());
    let has_chart_data = RwSignal::new(false);

    Effect::new(move || {
        if let Some(Ok(data)) = overview_resource.get() {
            let req_filled = fill_daily_range(
                &aggregate_daily(&data.request_buckets),
                start_ts.get_untracked(),
                end_ts.get_untracked(),
            );
            let tok_filled = fill_daily_range(
                &aggregate_daily(&data.token_buckets),
                start_ts.get_untracked(),
                end_ts.get_untracked(),
            );

            req_x_data.set(
                req_filled.iter().map(|r| ts_to_date_str(r.timestamp)).collect(),
            );
            req_series.set(vec![ChartSeries {
                name: "Requests".to_string(),
                data: req_filled.iter().map(|r| r.count as f64).collect(),
            }]);
            tok_x_data.set(
                tok_filled.iter().map(|r| ts_to_date_str(r.timestamp)).collect(),
            );
            tok_series.set(vec![ChartSeries {
                name: "Tokens".to_string(),
                data: tok_filled.iter().map(|r| r.count as f64).collect(),
            }]);
            model_pie_data.set(
                data.model_dist
                    .iter()
                    .map(|m| PieData {
                        name: m.model.clone(),
                        value: m.count as f64,
                    })
                    .collect(),
            );
            token_pie_data.set(
                data.token_dist
                    .iter()
                    .map(|e| PieData {
                        name: e.category.clone(),
                        value: e.count as f64,
                    })
                    .collect(),
            );
            has_chart_data.set(true);
        } else {
            has_chart_data.set(false);
        }
    });

    let content = move || match overview_resource.get() {
        None => overview_skeleton().into_any(),
        Some(Err(e)) => view! { <ErrorCard message=e.clone() /> }.into_any(),
        Some(Ok(data)) => view! {
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-6">
                <StatCard
                    icon="fa-server"
                    icon_bg="bg-indigo-100 text-indigo-600 dark:bg-indigo-900 dark:text-indigo-400"
                    value=data.provider_count.to_string()
                    label=t!(Providers)
                />
                <StatCard
                    icon="fa-arrow-right-arrow-left"
                    icon_bg="bg-amber-100 text-amber-600 dark:bg-amber-900 dark:text-amber-400"
                    value=data.api_request_count.to_string()
                    label=t!(ApiRequestCount)
                />
                <StatCard
                    icon="fa-gauge-high"
                    icon_bg="bg-teal-100 text-teal-600 dark:bg-teal-900 dark:text-teal-400"
                    value=format!("{:.2}", data.rpm)
                    // Keeping RPM/TPM as plain-text per convention
                    // (i18n keys exist but are intentionally unused)
                    label="RPM"
                />
            </div>
            <div class="grid grid-cols-1 sm:grid-cols-3 gap-6">
                <StatCard
                    icon="fa-cube"
                    icon_bg="bg-green-100 text-green-600 dark:bg-green-900 dark:text-green-400"
                    value=data.model_count.to_string()
                    label=t!(Models)
                />
                <StatCard
                    icon="fa-code"
                    icon_bg="bg-pink-100 text-pink-600 dark:bg-pink-900 dark:text-pink-400"
                    value=data.token_consumption.to_string()
                    label=t!(TokenConsumption)
                />
                <StatCard
                    icon="fa-fire"
                    icon_bg="bg-red-100 text-red-600 dark:bg-red-900 dark:text-red-400"
                    value=format!("{:.2}", data.tpm)
                    // Keeping RPM/TPM as plain-text per convention
                    // (i18n keys exist but are intentionally unused)
                    label="TPM"
                />
            </div>
        }.into_any(),
    };

    let greeting = {
        let user_name = auth.username.get_untracked().unwrap_or_default();
        tr!(Greeting, &[("username", &user_name)])
    };

    let start_str = Signal::derive(move || {
        let t = start_ts.get();
        if t == 0 {
            String::new()
        } else {
            ts_to_date_str(t)
        }
    });
    let end_str = Signal::derive(move || {
        let t = end_ts.get();
        if t == 0 {
            String::new()
        } else {
            ts_to_date_str(t)
        }
    });

    view! {
        <div class="space-y-6 sm:space-y-8">
            <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4">
                <h2 class="text-xl font-semibold text-gray-800 dark:text-gray-200">{greeting}</h2>
                <div class="flex flex-wrap items-center gap-2">
                    <button
                        class="px-3 py-1.5 text-sm font-medium rounded-lg bg-gray-100 dark:bg-gray-700 \
                        text-gray-600 dark:text-gray-300 \
                        hover:bg-gray-200 dark:hover:bg-gray-600 cursor-pointer"
                        on:click=move |_| {
                            let now = now_timestamp();
                            let today = midnight_ts(now);
                            set_range(today - 6 * 86400, now);
                        }
                    >
                        {t!(Last7Days)}
                    </button>
                    <button
                        class="px-3 py-1.5 text-sm font-medium rounded-lg bg-gray-100 dark:bg-gray-700 \
                        text-gray-600 dark:text-gray-300 \
                        hover:bg-gray-200 dark:hover:bg-gray-600 cursor-pointer"
                        on:click=move |_| {
                            let now = now_timestamp();
                            let today = midnight_ts(now);
                            set_range(today - 29 * 86400, now);
                        }
                    >
                        {t!(Last30Days)}
                    </button>
                    <input
                        type="date"
                        id="filter-start-date"
                        name="filter-start-date"
                        aria-label=t!(StartDate)
                        prop:value=move || start_str.get()
                        class="px-2 py-1.5 text-sm border rounded-lg bg-white dark:bg-gray-700 \
                        border-gray-300 dark:border-gray-600 \
                        text-gray-700 dark:text-gray-200 cursor-pointer"
                        on:change=on_start_date
                    />
                    <input
                        type="date"
                        id="filter-end-date"
                        name="filter-end-date"
                        aria-label=t!(EndDate)
                        prop:value=move || end_str.get()
                        class="px-2 py-1.5 text-sm border rounded-lg bg-white dark:bg-gray-700 \
                        border-gray-300 dark:border-gray-600 \
                        text-gray-700 dark:text-gray-200 cursor-pointer"
                        on:change=on_end_date
                    />
                    <button
                        class="p-2 rounded-lg bg-gray-100 dark:bg-gray-700 \
                        text-gray-600 dark:text-gray-300 \
                        hover:bg-gray-200 dark:hover:bg-gray-600 cursor-pointer"
                        on:click=move |_| refresh()
                    >
                        <i class="fas fa-sync-alt"></i>
                    </button>
                </div>
            </div>
            {content}
            <Show when=move || has_chart_data.get()>
                <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
                    <div class=format!("{} p-6", CLASS_CARD)>
                        <div class="flex items-center gap-2 mb-4">
                            <TabButton
                                active=left_tab.get() == 0
                                on_click={
                                    let l = set_left_tab;
                                    move || l.set(0)
                                }
                                label=t!(ApiRequestCountTrendingTableTitle)
                            />
                            <TabButton
                                active=left_tab.get() == 1
                                on_click={
                                    let l = set_left_tab;
                                    move || l.set(1)
                                }
                                label=t!(ModelDistribution)
                            />
                        </div>
                        <div style:display=move || {
                            if left_tab.get() == 0 { "block" } else { "none" }
                        }>
                            <LineChart
                                id="chart-left-line"
                                x_data=Signal::from(req_x_data)
                                series_list=Signal::from(req_series)
                            />
                        </div>
                        <div style:display=move || {
                            if left_tab.get() == 1 { "block" } else { "none" }
                        }>
                            <PieChart id="chart-left-pie" data=Signal::from(model_pie_data) />
                        </div>
                    </div>
                    <div class=format!("{} p-6", CLASS_CARD)>
                        <div class="flex items-center gap-2 mb-4">
                            <TabButton
                                active=right_tab.get() == 0
                                on_click={
                                    let l = set_right_tab;
                                    move || l.set(0)
                                }
                                label=t!(TokenConsumptionTrendingTableTitle)
                            />
                            <TabButton
                                active=right_tab.get() == 1
                                on_click={
                                    let l = set_right_tab;
                                    move || l.set(1)
                                }
                                label=t!(TokenDistribution)
                            />
                        </div>
                        <div style:display=move || {
                            if right_tab.get() == 0 { "block" } else { "none" }
                        }>
                            <LineChart
                                id="chart-right-line"
                                x_data=Signal::from(tok_x_data)
                                series_list=Signal::from(tok_series)
                            />
                        </div>
                        <div style:display=move || {
                            if right_tab.get() == 1 { "block" } else { "none" }
                        }>
                            <PieChart id="chart-right-pie" data=Signal::from(token_pie_data) />
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
