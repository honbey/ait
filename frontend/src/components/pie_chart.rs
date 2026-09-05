use leptos::prelude::*;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::components::echarts;

#[derive(Clone, Serialize, PartialEq)]
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

    // Dark values track the neutral `ink` ramp; see tailwind.css.
    let label_color = if dark { "#d5d5d5" } else { "#374151" };
    let tooltip_bg = if dark { "#282828" } else { "#ffffff" };
    let tooltip_border = if dark { "#404040" } else { "#e5e7eb" };
    let tooltip_color = if dark { "#e7e7e7" } else { "#374151" };

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
pub fn PieChart(id: &'static str, data: Signal<Vec<PieData>>) -> impl IntoView {
    let node = NodeRef::<leptos::html::Div>::new();
    let chart = echarts::use_chart(node);
    let dark = use_context::<Memo<bool>>();

    Effect::new(move || {
        let dark = dark.map(|d| d.get()).unwrap_or(false);
        let option = build_pie_option(&data.get(), dark);
        if let Some(c) = chart.get() {
            c.set_option(&option);
        }
    });

    view! { <div id=id node_ref=node class="h-64 w-full"></div> }
}
