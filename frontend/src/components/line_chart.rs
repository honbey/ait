use std::cell::Cell;
use std::rc::Rc;

use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::tags::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::chart::{Chart, ChartSeries, build_line_option};

fn try_init_chart(
    id: &str,
    x_data: &[String],
    series_list: &[ChartSeries],
    chart_w: &Signal<Option<Chart>>,
) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let c = Chart::new(&el);
        let option = build_line_option(x_data, series_list);
        c.set_option(&option);
        chart_w.set(Some(c));
    }
}

#[derive(Props)]
pub struct LineChartProps {
    pub id: &'static str,
    pub x_data: Vec<String>,
    pub series_list: Vec<ChartSeries>,
}

#[component]
pub fn LineChart(props: LineChartProps) -> View {
    let chart = create_signal(None::<Chart>);
    let mounted = Rc::new(Cell::new(true));
    let onload_fn = create_signal(None::<JsValue>);
    let script_el = create_signal(None::<web_sys::Element>);

    let id = props.id;
    let x_data = props.x_data.clone();
    let series_list = props.series_list.clone();
    let m = mounted.clone();

    let handle = Timeout::new(0, {
        move || {
            if !m.get() {
                return;
            }

            // ECharts already loaded → init immediately
            if js_sys::Reflect::has(&js_sys::global(), &"echarts".into()).unwrap_or(false) {
                try_init_chart(id, &x_data, &series_list, &chart);
                return;
            }

            let document = web_sys::window().unwrap().document().unwrap();

            // Reuse existing script tag if another LineChart already injected it
            let existing = document
                .query_selector("script[src='/echarts.min.js']")
                .ok()
                .flatten();

            let script = existing.unwrap_or_else(|| {
                let s = document.create_element("script").unwrap();
                let _ = s.set_attribute("src", "/echarts.min.js");
                document.head().unwrap().append_child(&s).unwrap();
                s
            });
            script_el.set(Some(script.clone()));

            let cb_id = id;
            let cb_x_data = x_data.clone();
            let cb_series = series_list.clone();
            let cb_m = m.clone();

            let onload: Closure<dyn FnMut()> = Closure::new(move || {
                if !cb_m.get() {
                    return;
                }
                try_init_chart(cb_id, &cb_x_data, &cb_series, &chart);
            });

            script
                .add_event_listener_with_callback("load", onload.as_ref().unchecked_ref())
                .unwrap();
            onload_fn.set(Some(onload.into_js_value()));
        }
    });

    let m2 = mounted;
    on_cleanup(move || {
        m2.set(false);
        drop(handle);
        if let Some(ref script) = script_el.get_clone()
            && let Some(ref f) = onload_fn.get_clone()
            && let Some(cb) = f.dyn_ref::<js_sys::Function>()
        {
            let _ = script.remove_event_listener_with_callback("load", cb);
        }

        onload_fn.set(None);
        script_el.set(None);
        if let Some(c) = chart.get_clone() {
            c.dispose();
        }
    });

    div().attr("id", props.id).class("h-64 w-full").into()
}
