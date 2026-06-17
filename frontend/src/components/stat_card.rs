use sycamore::prelude::*;
use sycamore::web::tags::*;

#[derive(Props)]
pub struct StatCardProps {
    pub icon: String,
    pub bg_color: String,
    pub value: String,
    pub label: String,
}

#[component]
pub fn StatCard(props: StatCardProps) -> View {
    div()
        .class("bg-white dark:bg-gray-800 rounded-xl p-6 flex items-center gap-4 shadow-sm")
        .children((
            div()
                .class(format!(
                    "w-14 h-14 rounded-full flex items-center justify-center text-xl {}",
                    props.bg_color
                ))
                .children(i().class(format!("fas {}", props.icon))),
            div().children((
                div()
                    .class("text-3xl font-bold text-gray-800 dark:text-gray-100")
                    .children(props.value),
                div()
                    .class("text-sm text-gray-500 dark:text-gray-400")
                    .children(props.label),
            )),
        ))
        .into()
}
