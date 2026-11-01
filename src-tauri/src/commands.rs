//! Tauri-Commands: Funktionen, die das Frontend mit `invoke("name")` aufrufen kann.

use std::time::Instant;

use std::sync::atomic::Ordering;

use bootforge_core::acquire::{self, AcquireEvent, Acquired};
use bootforge_core::catalog::load::{self, DEFAULT_CATALOG_URL, Loaded};
use bootforge_core::resolve::{self, ResolvedImage};
use tauri::ipc::Channel;
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
    resolve_cached(&state, variant_id, force.unwrap_or(false)).await
}

async fn resolve_cached(
    state: &AppState,
    variant_id: String,
    force: bool,
) -> Result<ResolvedImage, AppError> {
    if !force {
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

/// Lädt das Image einer Variante herunter und prüft es vollständig.
///
/// Fortschritt geht über einen `Channel` an die GUI. Channels sind in Tauri v2 der empfohlene
/// Weg für viele, schnelle Nachrichten an *einen* Empfänger (statt globaler Events an alle Fenster).
#[tauri::command]
pub async fn prepare_image(
    variant_id: String,
    on_event: Channel<AcquireEvent>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Acquired, AppError> {
    // `swap` setzt atomar auf true und liefert den alten Wert: war schon true → läuft bereits.
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err(AppError::Busy);
    }
    // Guard: setzt `busy` zurück, egal wie die Funktion endet (auch bei `?`).
    struct Done<'a>(&'a AppState);
    impl Drop for Done<'_> {
        fn drop(&mut self) {
            self.0.busy.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done(&state);
    state.cancel.store(false, Ordering::SeqCst);

    let img = resolve_cached(&state, variant_id, false).await?;
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .join("images");
    let result = acquire::acquire(&state.http, &img, &dir, &state.cancel, |e| {
        // Ist das Fenster zu, gibt es keinen Empfänger mehr – das ist kein Fehler.
        let _ = on_event.send(e);
    })
    .await?;
    Ok(result)
}

/// Bricht den laufenden Download ab. Die Teil-Datei bleibt erhalten (Fortsetzen möglich).
#[tauri::command]
pub fn cancel_prepare(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
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
