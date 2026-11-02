//! Commands für Images, die der Nutzer selbst mitbringt:
//! Windows-ISOs von Microsofts Seite und beliebige eigene Images („Eigenes Image“).
//!
//! Sicherheit: Dateiauswahl und das Öffnen der Download-Seite laufen hier im Backend.
//! Das Frontend bekommt dadurch keinerlei Dateisystem- oder „Programm öffnen“-Rechte
//! (Capabilities bleiben bei `core:default`), und die geöffnete Adresse stammt immer aus dem
//! Katalog – nie aus dem Frontend.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use bootforge_core::acquire::{AcquireEvent, Acquired};
use bootforge_core::catalog::{Checksum, FlashMode, Source, Variant};
use bootforge_core::local::{self, Expected, ImageInfo};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::error::AppError;
use crate::state::{AppState, BusyGuard};

/// Ergebnis von `verify_local`: geprüfte Datei + wie sie geschrieben wird.
#[derive(Debug, Clone, Serialize)]
pub struct LocalImage {
    pub acquired: Acquired,
    pub info: ImageInfo,
}

fn variant(state: &AppState, id: &str) -> Result<Variant, AppError> {
    let catalog = state.catalog.lock().expect("Mutex nicht vergiftet");
    let catalog = catalog.as_ref().ok_or(AppError::CatalogNotLoaded)?;
    catalog
        .distros
        .iter()
        .flat_map(|d| &d.variants)
        .find(|v| v.id == id)
        .cloned()
        .ok_or_else(|| AppError::UnknownVariant(id.to_owned()))
}

/// Datei-Auswahldialog des Betriebssystems. `None`, wenn der Nutzer abbricht.
#[tauri::command]
pub async fn pick_image(app: AppHandle) -> Result<Option<PathBuf>, AppError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Images", &["iso", "img", "raw", "xz", "gz", "zst"])
        .add_filter("*", &["*"])
        .pick_file(move |f| {
            let _ = tx.send(f);
        });
    let picked = rx.await.map_err(|e| AppError::Internal(e.to_string()))?;
    picked
        .map(|f| f.into_path().map_err(|e| AppError::Internal(e.to_string())))
        .transpose()
}

/// Öffnet die Download-Seite einer Variante mit manueller Quelle im Standardbrowser.
#[tauri::command]
pub fn open_download_page(
    variant_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let v = variant(&state, &variant_id)?;
    let Source::Manual { download_page, .. } = v.source else {
        return Err(AppError::UnknownVariant(variant_id));
    };
    // Der Katalog-Validator hat schon HTTPS erzwungen – trotzdem hier noch einmal.
    let url = url::Url::parse(&download_page)
        .ok()
        .filter(|u| u.scheme() == "https")
        .ok_or_else(|| AppError::Insecure(download_page.clone()))?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// Prüft eine vorhandene Datei.
///
/// - `variant_id` gesetzt (Windows): Der Hash muss in Microsofts Tabelle stehen (Adresse aus
///   dem Katalog), und die Datei muss ein Windows-Installationsmedium sein.
/// - sonst („Eigenes Image“): `hash` ist Pflicht – vom Hersteller kopiert.
///
/// Das Ergebnis merkt sich das Backend; „Forge!“ akzeptiert nur so geprüfte Dateien
/// (und prüft beim Schreiben ohnehin noch einmal).
#[tauri::command]
pub async fn verify_local(
    path: PathBuf,
    variant_id: Option<String>,
    hash: Option<String>,
    on_event: Channel<AcquireEvent>,
    state: State<'_, AppState>,
) -> Result<LocalImage, AppError> {
    let _busy = BusyGuard::acquire(&state)?;
    state.cancel.store(false, Ordering::SeqCst);

    let expected = match &variant_id {
        Some(id) => match variant(&state, id)?.checksum {
            Checksum::PageListing { url, algorithm } => Expected::PageListing {
                url: url::Url::parse(&url).map_err(|_| AppError::Insecure(url.clone()))?,
                algorithm,
            },
            _ => return Err(AppError::UnknownVariant(id.clone())),
        },
        None => {
            let (algorithm, value) = hash
                .as_deref()
                .and_then(local::parse_user_hash)
                .ok_or(AppError::InvalidHash)?;
            Expected::Hash { algorithm, value }
        }
    };

    // Erst ansehen (schnell), dann hashen (dauert): Ein Linux-ISO unter „Windows 11“ soll
    // nicht erst nach Minuten auffallen.
    let p = path.clone();
    let info = tauri::async_runtime::spawn_blocking(move || local::inspect(&p))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|e| AppError::Disk(format!("{}: {e}", path.display())))?;
    if variant_id.is_some() && info.flash_mode != FlashMode::Windows {
        return Err(AppError::NotWindowsMedia);
    }

    let acquired = local::verify_local(&state.http, &path, &expected, &state.cancel, |e| {
        let _ = on_event.send(e);
    })
    .await
    .map_err(|e| match e {
        // Eigene, klarere Meldung: Die Datei ist keine unveränderte Original-ISO.
        bootforge_core::acquire::AcquireError::ChecksumNotListed { .. } if variant_id.is_some() => {
            AppError::NotOnVendorList(e.to_string())
        }
        e => e.into(),
    })?;

    let img = LocalImage { acquired, info };
    state
        .local
        .lock()
        .expect("Mutex nicht vergiftet")
        .insert(path, img.clone());
    Ok(img)
}
