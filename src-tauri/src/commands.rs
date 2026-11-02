//! Tauri-Commands: Funktionen, die das Frontend mit `invoke("name")` aufrufen kann.

use std::path::PathBuf;
use std::time::Instant;

use std::sync::atomic::Ordering;

use bootforge_core::acquire::{self, AcquireEvent, Acquired};
use bootforge_core::catalog::load::{self, DEFAULT_CATALOG_URL, Loaded};
use bootforge_core::device::{self, Drive, ScanOptions};
use bootforge_core::flash::{FlashJob, FlashReport, ForgeRequest};
use bootforge_core::resolve::{self, ResolvedImage};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use url::Url;

use crate::error::AppError;
use crate::events::ForgeEvent;
use crate::state::{AppState, BusyGuard, RESOLVE_TTL};

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
        if let Some((at, img)) = cache.get(&variant_id)
            && at.elapsed() < RESOLVE_TTL
        {
            return Ok(img.clone());
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
    let _busy = BusyGuard::acquire(&state)?;
    state.cancel.store(false, Ordering::SeqCst);

    let img = resolve_cached(&state, variant_id, false).await?;
    let dir = images_dir(&app)?;
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

/// Ergebnis von „Forge!“.
#[derive(serde::Serialize)]
pub struct ForgeResult {
    pub acquired: Acquired,
    pub report: FlashReport,
}

/// Woher das Image für „Forge!“ kommt.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ForgeSource {
    /// Aus dem Katalog: wird (falls nötig) heruntergeladen und geprüft.
    Catalog { variant_id: String },
    /// Eigene Datei, vorher mit `verify_local` geprüft.
    Local { path: PathBuf },
}

/// „Forge!“: Image beschaffen + prüfen (bzw. die geprüfte eigene Datei nehmen), dann auf den
/// Stick schreiben.
///
/// `confirm_name` ist das, was der Nutzer im Bestätigungsdialog eingetippt hat. Es wird HIER
/// noch einmal geprüft – ein Fehler in der GUI darf die Bestätigung nicht aushebeln.
#[tauri::command]
pub async fn forge(
    source: ForgeSource,
    drive: Drive,
    show_external_hdds: bool,
    confirm_name: String,
    on_event: Channel<ForgeEvent>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ForgeResult, AppError> {
    if !confirm_name.trim().eq_ignore_ascii_case(drive.name.trim()) {
        return Err(AppError::ConfirmMismatch);
    }
    let _busy = BusyGuard::acquire(&state)?;
    state.cancel.store(false, Ordering::SeqCst);

    let (acquired, compression, mode) = match source {
        ForgeSource::Catalog { variant_id } => {
            let img = resolve_cached(&state, variant_id, false).await?;
            let dir = images_dir(&app)?;
            let acquired = acquire::acquire(&state.http, &img, &dir, &state.cancel, |e| {
                let _ = on_event.send(ForgeEvent::Acquire(e));
            })
            .await?;
            (acquired, img.compression, img.flash_mode)
        }
        ForgeSource::Local { path } => {
            let local = state
                .local
                .lock()
                .expect("Mutex nicht vergiftet")
                .get(&path)
                .cloned()
                .ok_or(AppError::NotVerified)?;
            (
                local.acquired,
                local.info.compression,
                local.info.flash_mode,
            )
        }
    };

    let req = ForgeRequest {
        drive,
        opts: ScanOptions { show_external_hdds },
        job: FlashJob {
            image: acquired.path.clone(),
            compression,
            algorithm: acquired.algorithm,
            expected_hash: acquired.hash.clone(),
        },
        mode,
    };
    let report = crate::forge::flash(req, state.cancel.clone(), on_event).await?;
    Ok(ForgeResult { acquired, report })
}

fn images_dir(app: &AppHandle) -> Result<std::path::PathBuf, AppError> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .join("images"))
}

/// Wie viel Platz belegen heruntergeladene Images?
#[tauri::command]
pub async fn cache_info(app: AppHandle) -> Result<bootforge_core::disk::CacheInfo, AppError> {
    let dir = images_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || bootforge_core::disk::cache_info(&dir))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Disk(e.to_string()))
}

/// Löscht alle heruntergeladenen Images (nicht während eines Downloads oder Schreibvorgangs).
#[tauri::command]
pub async fn clear_cache(app: AppHandle, state: State<'_, AppState>) -> Result<u64, AppError> {
    let _busy = BusyGuard::acquire(&state)?;
    let dir = images_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || bootforge_core::disk::clear_cache(&dir))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Disk(e.to_string()))
}

/// Listet alle Laufwerke mit Urteil (wählbar oder nicht, mit Grund).
///
/// Die Erkennung ist synchroner Code (unter Windows muss die WMI-Verbindung im selben Thread
/// bleiben). `spawn_blocking` verlegt sie in einen eigenen Thread, damit die App nicht hängt.
#[tauri::command]
pub async fn list_drives(show_external_hdds: Option<bool>) -> Result<Vec<Drive>, AppError> {
    let opts = ScanOptions {
        show_external_hdds: show_external_hdds.unwrap_or(false),
    };
    tauri::async_runtime::spawn_blocking(move || {
        // Nur in Debug-Builds: nachgebautes /sys zum Testen der GUI ohne echten Stick.
        // In Release-Builds existiert dieser Zweig gar nicht.
        #[cfg(all(debug_assertions, target_os = "linux"))]
        if let Ok(root) = std::env::var("BOOTFORGE_DEV_SYSROOT") {
            return device::list_from_root(std::path::Path::new(&root), opts);
        }
        device::list(opts)
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?
    .map_err(|e| AppError::DeviceScan(e.to_string()))
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
