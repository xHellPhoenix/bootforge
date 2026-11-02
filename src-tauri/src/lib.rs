//! Tauri-App: verbindet GUI und `bootforge-core`.
//!
//! Hier steht absichtlich wenig Logik. Commands nehmen Anfragen der GUI entgegen,
//! rufen `bootforge-core` auf und geben Ergebnisse (oder Fehler) zurück.

mod commands;
mod error;
mod state;

use bootforge_core::http::HttpClient;

pub fn run() {
    let http = HttpClient::new().expect("HTTP-Client konnte nicht erstellt werden");

    tauri::Builder::default()
        .manage(state::AppState::new(http))
        .invoke_handler(tauri::generate_handler![
            commands::get_catalog,
            commands::resolve_latest,
            commands::prepare_image,
            commands::cancel_prepare,
            commands::app_info
        ])
        .run(tauri::generate_context!())
        .expect("BootForge konnte nicht gestartet werden");
}
