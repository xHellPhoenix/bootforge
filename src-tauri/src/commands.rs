//! Tauri-Commands: Funktionen, die das Frontend mit `invoke("name")` aufrufen kann.

use std::time::Instant;

use bootforge_core::catalog::load::{self, DEFAULT_CATALOG_URL, Loaded};
use bootforge_core::resolve::{self, ResolvedImage};
use tauri::{AppHandle, Manager, State};
use url::Url;

use crate::error::AppError;
use crate::state::{AppState, RESOLVE_TTL};

/// Katalog-URL. Für Tests/Entwicklung per Umgebungsvariable `BOOTFORGE_CATALOG_URL`
/// überschreibbar – aber nur mit HTTPS, sonst wird die Standard-URL genommen.
fn catalog_url() -> Url {
    std::env::var("BOOTFORGE_CATALOG_URL")
        .ok()
        .and_then(|s| Url::parse(&s).ok())
        .filter(|u| u.scheme() == "https")
        .unwrap_or_else(|| DEFAULT_CATALOG_URL.parse().expect("gültige Standard-URL"))
}

/// Lädt den Katalog (online → Cache → eingebaut). Schlägt nie fehl.
#[tauri::command]
pub async fn get_catalog(app: AppHandle, state: State<'_, AppState>) -> Result<Loaded, AppError> {
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let loaded = load::load(&state.http, &catalog_url(), &cache_dir).await;

    *state.catalog.lock().expect("Mutex nicht vergiftet") = Some(loaded.catalog.clone());
    // Neuer Katalog → alte Ergebnisse könnten auf veralteten Regeln beruhen.
    state
        .resolved
        .lock()
        .expect("Mutex nicht vergiftet")
        .clear();
    Ok(loaded)
}

/// Ermittelt die neueste Version einer Variante.
#[tauri::command]
pub async fn resolve_latest(
    variant_id: String,
    force: Option<bool>,
    state: State<'_, AppState>,
) -> Result<ResolvedImage, AppError> {
    if !force.unwrap_or(false) {
        let cache = state.resolved.lock().expect("Mutex nicht vergiftet");
        if let Some((at, img)) = cache.get(&variant_id) {
            if at.elapsed() < RESOLVE_TTL {
                return Ok(img.clone());
            }
        }
    }

    // Variante kopieren, damit wir den Lock nicht über das `.await` halten.
    let variant = {
        let catalog = state.catalog.lock().expect("Mutex nicht vergiftet");
        let catalog = catalog.as_ref().ok_or(AppError::CatalogNotLoaded)?;
        catalog
            .distros
            .iter()
            .flat_map(|d| &d.variants)
            .find(|v| v.id == variant_id)
            .cloned()
            .ok_or_else(|| AppError::UnknownVariant(variant_id.clone()))?
    };

    let img = resolve::resolve(&state.http, &variant).await?;
    state
        .resolved
        .lock()
        .expect("Mutex nicht vergiftet")
        .insert(variant_id, (Instant::now(), img.clone()));
    Ok(img)
}

#[derive(serde::Serialize)]
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
