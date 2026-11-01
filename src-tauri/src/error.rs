use serde::Serialize;

/// Fehler, wie sie an die GUI gehen.
///
/// Statt nur eines Textes schicken wir einen `code` mit. Die GUI übersetzt den Code
/// (Deutsch/Englisch) und kann `detail` für Techniker aufklappbar anzeigen.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum AppError {
    #[error("Katalog ungültig: {0}")]
    CatalogInvalid(String),
}

impl From<bootforge_core::catalog::CatalogError> for AppError {
    fn from(e: bootforge_core::catalog::CatalogError) -> Self {
        AppError::CatalogInvalid(e.to_string())
    }
}
