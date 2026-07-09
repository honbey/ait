use leptos::prelude::*;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::components::echarts;

#[derive(Clone, serde::Serialize)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
}

fn build_line_option(x_data: &[String], series_list: &[ChartSeries]) -> JsValue {
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

    let option = serde_json::json!({
        "tooltip": { "trigger": "axis" },
        "grid": { "left": "5%", "right": "5%", "top": "5%", "bottom": "5%", "containLabel": true },
        "xAxis": {
            "type": "category",
            "data": x_data,
            "boundaryGap": false,
        },
        "yAxis": { "type": "value", "min": 0 },
        "series": series_json,
    });

    option
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap()
}

#[component]
pub fn LineChart(
    id: &'static str,
    x_data: Vec<String>,
    series_list: Vec<ChartSeries>,
) -> impl IntoView {
    let (chart, mounted) = echarts::use_echarts(id);
    let option = build_line_option(&x_data, &series_list);
    echarts::init_or_show_chart(id, chart, mounted, option);

    view! { <div id=id class="h-64 w-full"></div> }
}
