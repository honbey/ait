use sycamore_router::Route;

#[derive(Route, Clone, Copy, Debug, PartialEq)]
pub enum AppRoute {
    #[to("/")]
    Index,
    #[to("/login")]
    Login,
    #[to("/register")]
    Register,
    #[to("/console/dashboard")]
    Dashboard,
    #[to("/console/providers")]
    Providers,
    #[to("/console/models")]
    Models,
    #[to("/console/api-keys")]
    ApiKeys,
    #[to("/console/text-generation")]
    TextGeneration,
    #[not_found]
    NotFound,
}

impl AppRoute {
    pub fn is_console(&self) -> bool {
        matches!(
            self,
            Self::Dashboard | Self::Providers | Self::Models | Self::ApiKeys | Self::TextGeneration
        )
    }
}
