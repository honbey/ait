use gloo_timers::callback::Timeout;
use sycamore::prelude::*;
use sycamore::web::tags::*;
use wasm_bindgen::JsCast;

use crate::chart::{build_line_option, Chart, ChartSeries};

#[derive(Props)]
pub struct LineChartProps {
    pub id: &'static str,
    pub x_data: Vec<String>,
    pub series_list: Vec<ChartSeries>,
}

#[component]
pub fn LineChart(props: LineChartProps) -> View {
    let chart = create_signal(None::<Chart>);

    // Defer init after DOM mount via microtask
    let id = props.id;
    let x_data = props.x_data.clone();
    let series_list = props.series_list.clone();
    let chart_w = chart.clone();
    Timeout::new(0, move || {
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(id))
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let c = Chart::new(&el);
            let option = build_line_option(&x_data, &series_list);
            c.set_option(&option);
            chart_w.set(Some(c));
        }
    })
    .forget();

    on_cleanup(move || {
        if let Some(c) = chart.get_clone() {
            c.dispose();
        }
    });

    div()
        .attr("id", props.id)
        .class("h-64 w-full")
        .into()
}
