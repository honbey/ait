use leptos::prelude::*;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::components::echarts;

#[derive(Clone, Serialize)]
pub struct PieData {
    pub name: String,
    pub value: f64,
}

fn build_pie_option(data: &[PieData], dark: bool) -> JsValue {
    let data_json: Vec<serde_json::Value> = data
        .iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name,
                "value": d.value,
            })
        })
        .collect();

    let label_color = if dark { "#d1d5db" } else { "#374151" };
    let tooltip_bg = if dark { "#1f2937" } else { "#ffffff" };
    let tooltip_border = if dark { "#374151" } else { "#e5e7eb" };
    let tooltip_color = if dark { "#e5e7eb" } else { "#374151" };

    let option = serde_json::json!({
        // Catppuccin Frappe (transposed: 4,4,3,3)
        // Rosewater, Red,     Green, Blue
        // Flamingo,  Maroon,  Teal,  Lavender
        // Pink,      Peach,   Sky
        // Mauve,     Yellow,  Sapphire
        "color": ["#f2d5cf", "#e78284", "#a6d189", "#8caaee",
                  "#eebebe", "#ea999c", "#81c8be", "#babbf1",
                  "#f4b8e4", "#ef9f76", "#99d1db",
                  "#ca9ee6", "#e5c890", "#85c1dc"],
        "tooltip": {
            "trigger": "item",
            "formatter": "{b}: {c} ({d}%)",
            "backgroundColor": tooltip_bg,
            "borderColor": tooltip_border,
            "textStyle": { "color": tooltip_color }
        },
        "series": [{
            "type": "pie",
            "radius": ["40%", "70%"],
            "avoidLabelOverlap": true,
            "label": {
                "show": true,
                "color": label_color,
                "formatter": "{b}: {d}%"
            },
            "emphasis": {
                "label": {
                    "show": true,
                    "fontSize": 14,
                    "fontWeight": "bold"
                }
            },
            "data": data_json,
        }],
    });

    option
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .unwrap()
}

#[component]
pub fn PieChart(id: &'static str, data: Vec<PieData>) -> impl IntoView {
    let dark = use_context::<RwSignal<bool>>()
        .map(|d| d.get_untracked())
        .unwrap_or(false);
    let (chart, mounted) = echarts::use_echarts(id);
    let option = build_pie_option(&data, dark);
    echarts::init_or_show_chart(id, chart, mounted, option);

    view! { <div id=id class="h-64 w-full"></div> }
}
