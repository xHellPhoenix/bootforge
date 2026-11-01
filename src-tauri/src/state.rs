//! Zustand der App, den sich alle Commands teilen (`tauri::State`).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use bootforge_core::catalog::Catalog;
use bootforge_core::http::HttpClient;
use bootforge_core::resolve::ResolvedImage;

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
}

impl AppState {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            catalog: Mutex::new(None),
            resolved: Mutex::new(HashMap::new()),
        }
    }
}
