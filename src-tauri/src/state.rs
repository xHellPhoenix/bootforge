//! Zustand der App, den sich alle Commands teilen (`tauri::State`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sealflash_core::catalog::Catalog;
use sealflash_core::http::HttpClient;
use sealflash_core::resolve::ResolvedImage;

use crate::error::AppError;
use crate::local::LocalImage;

/// Wie lange ein ermitteltes Ergebnis wiederverwendet wird, bevor erneut nachgesehen wird.
/// Spart Anfragen, wenn man in der GUI zwischen Varianten hin- und herklickt.
pub const RESOLVE_TTL: Duration = Duration::from_secs(15 * 60);

pub struct AppState {
    pub http: HttpClient,
    /// Der aktuell geladene Katalog (nach `get_catalog`).
    ///
    /// `std::sync::Mutex` reicht, weil wir ihn nie über ein `.await` hinweg halten –
    /// nur kurz Daten rein oder raus kopieren.
    pub catalog: Mutex<Option<Catalog>>,
    pub resolved: Mutex<HashMap<String, (Instant, ResolvedImage)>>,
    /// Läuft gerade ein Download? Es darf immer nur einer laufen.
    pub busy: AtomicBool,
    /// Abbruch-Signal für den laufenden Download. `Arc`, weil auch der Download-Task es hält.
    pub cancel: Arc<AtomicBool>,
    /// Selbst mitgebrachte, geprüfte Dateien (Pfad → Ergebnis). Nur diese nimmt „Forge!“ an.
    pub local: Mutex<HashMap<PathBuf, LocalImage>>,
}

/// Belegt `busy` für die Dauer eines Vorgangs und gibt es am Ende wieder frei –
/// egal wie die Funktion endet (auch bei `?` oder Panik).
pub struct BusyGuard<'a>(&'a AppState);

impl<'a> BusyGuard<'a> {
    pub fn acquire(state: &'a AppState) -> Result<Self, AppError> {
        // `swap` setzt atomar auf true und liefert den alten Wert: war schon true → läuft bereits.
        if state.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::Busy);
        }
        Ok(Self(state))
    }
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0
            .busy
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

impl AppState {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            catalog: Mutex::new(None),
            resolved: Mutex::new(HashMap::new()),
            busy: AtomicBool::new(false),
            cancel: Arc::new(AtomicBool::new(false)),
            local: Mutex::new(HashMap::new()),
        }
    }
}
