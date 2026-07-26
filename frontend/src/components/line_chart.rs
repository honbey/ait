use leptos::prelude::*;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::components::echarts;

#[derive(Clone, Serialize, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
}

fn build_line_option(x_data: &[String], series_list: &[ChartSeries], dark: bool) -> JsValue {
    let series_json: Vec<serde_json::Value> = series_list
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "type": "line",
                "data": s.data,
                "smooth": true,
                "symbol": "circle",
                "symbolSize": 6,
            })
        })
        .collect();

    let axis_color = if dark { "#d1d5db" } else { "#374151" };
    let split_color = if dark { "#374151" } else { "#e5e7eb" };
    let tooltip_bg = if dark { "#1f2937" } else { "#ffffff" };
    let tooltip_border = if dark { "#374151" } else { "#e5e7eb" };
    let tooltip_color = if dark { "#e5e7eb" } else { "#374151" };

    let option = serde_json::json!({
        "tooltip": {
            "trigger": "axis",
            "backgroundColor": tooltip_bg,
            "borderColor": tooltip_border,
            "textStyle": { "color": tooltip_color }
        },
        "grid": { "left": "5%", "right": "5%", "top": "5%", "bottom": "5%", "containLabel": true },
        "xAxis": {
            "type": "category",
            "data": x_data,
            "boundaryGap": false,
            "axisLabel": { "color": axis_color },
            "axisLine": { "lineStyle": { "color": split_color } },
            "splitLine": { "lineStyle": { "color": split_color } }
        },
        "yAxis": {
            "type": "value",
            "min": 0,
            "axisLabel": { "color": axis_color },
            "axisLine": { "show": false },
            "splitLine": { "lineStyle": { "color": split_color } }
        },
        "series": series_json,
    });

    option
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap()
}

#[component]
pub fn LineChart(
    id: &'static str,
    x_data: Signal<Vec<String>>,
    series_list: Signal<Vec<ChartSeries>>,
) -> impl IntoView {
    let node = NodeRef::<leptos::html::Div>::new();
    let chart = echarts::use_chart(node);
    let dark = use_context::<RwSignal<bool>>();

    Effect::new(move || {
        let option = build_line_option(
            &x_data.get(),
            &series_list.get(),
            dark.map(|d| d.get()).unwrap_or(false),
        );
        if let Some(c) = chart.get() {
            c.set_option(&option);
        }
    });

    view! { <div id=id node_ref=node class="h-64 w-full"></div> }
}
