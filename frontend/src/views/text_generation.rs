use sycamore::prelude::*;
use sycamore::web::tags::*;

use crate::components::text_generation::{TextGeneration, TextGenerationProps};
use crate::models::Model;

pub fn render_text_generation_view(models: Vec<Model>) -> View {
    div()
        .class("p-4 sm:p-8")
        .children(TextGeneration(TextGenerationProps { models }))
        .into()
}
