#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Route {
    Index,
    Login,
    Dashboard,
    Providers,
    Models,
    ApiKeys,
    TextGeneration,
}

impl Route {
    pub fn is_console(self) -> bool {
        matches!(
            self,
            Route::Dashboard
                | Route::Providers
                | Route::Models
                | Route::ApiKeys
                | Route::TextGeneration
        )
    }
}
