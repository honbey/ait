use sycamore::prelude::*;
use sycamore::web::events;
use sycamore::web::tags::*;

#[derive(Props)]
pub struct DarkModeToggleProps {
    pub dark: Signal<bool>,
}

#[component]
pub fn DarkModeToggle(props: DarkModeToggleProps) -> View {
    button()
        .class(
            "px-2 py-2 text-gray-500 dark:text-gray-400 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-pointer transition-colors",
        )
        .on(events::click, move |_| {
            props.dark.set(!props.dark.get());
        })
        .children(
            i().class(move || {
                if props.dark.get() {
                    "fas fa-moon w-4 text-center"
                } else {
                    "fas fa-sun w-4 text-center"
                }
            }),
        )
        .into()
}
