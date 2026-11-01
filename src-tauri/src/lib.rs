//! Tauri-App: verbindet GUI und `bootforge-core`.
//!
//! Hier steht absichtlich wenig Logik. Commands nehmen Anfragen der GUI entgegen,
//! rufen `bootforge-core` auf und geben Ergebnisse (oder Fehler) zurück.

mod commands;
mod error;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::get_catalog,
            commands::app_info
        ])
        .run(tauri::generate_context!())
        .expect("BootForge konnte nicht gestartet werden");
}
