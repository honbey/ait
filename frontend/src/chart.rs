use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[derive(Clone)]
pub struct Chart(JsValue);

impl Chart {
    pub fn new(dom: &web_sys::HtmlElement) -> Self {
        let echarts =
            js_sys::Reflect::get(&js_sys::global(), &"echarts".into()).expect("echarts not found");
        let init = js_sys::Reflect::get(&echarts, &"init".into()).expect("echarts.init not found");
        let f: js_sys::Function = init.unchecked_into();
        let result = f
            .call1(&JsValue::UNDEFINED, dom)
            .expect("echarts.init failed");
        Chart(result)
    }

    pub fn set_option(&self, option: &JsValue) {
        let set_option =
            js_sys::Reflect::get(&self.0, &"setOption".into()).expect("chart.setOption not found");
        let f: js_sys::Function = set_option.unchecked_into();
        let _ = f.call1(&self.0, option);
    }

    pub fn dispose(&self) {
        let dispose =
            js_sys::Reflect::get(&self.0, &"dispose".into()).expect("chart.dispose not found");
        let f: js_sys::Function = dispose.unchecked_into();
        let _ = f.call0(&self.0);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
}

pub fn build_line_option(x_data: &[String], series_list: &[ChartSeries]) -> JsValue {
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
