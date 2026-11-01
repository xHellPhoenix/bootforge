//! Tauri-Commands: Funktionen, die das Frontend mit `invoke("name")` aufrufen kann.

use bootforge_core::catalog::{self, Catalog, Issue};
use serde::Serialize;

use crate::error::AppError;

/// Antwort auf `get_catalog`.
#[derive(Serialize)]
pub struct CatalogResponse {
    pub catalog: Catalog,
    /// Woher der Katalog stammt (Phase 1: immer `bundled`; ab Phase 2 auch `remote`/`cache`).
    pub origin: &'static str,
    pub warnings: Vec<Issue>,
}

/// Liefert den Katalog. Phase 1: der eingebettete Katalog.
#[tauri::command]
pub fn get_catalog() -> Result<CatalogResponse, AppError> {
    let (catalog, warnings) = catalog::bundled()?;
    Ok(CatalogResponse {
        catalog,
        origin: "bundled",
        warnings,
    })
}

#[derive(Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}
