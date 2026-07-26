use leptos::prelude::*;

pub mod console_layout;
pub mod echarts;
pub mod error_display;
pub mod line_chart;
pub mod modal;
pub mod pie_chart;
pub mod sidebar;
pub mod skeleton;
pub mod style;
pub mod table;
pub mod toast;
pub mod topbar;

pub fn use_page_title(title: impl Fn() -> String + 'static) {
    Effect::new(move |_| {
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            doc.set_title(&title());
        }
    });
}
