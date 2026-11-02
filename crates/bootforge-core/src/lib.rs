//! # bootforge-core
//!
//! Die gesamte Logik von BootForge – bewusst **ohne** Abhängigkeit zu Tauri/GUI.
//! Vorteile:
//! - mit `cargo test` testbar, ohne ein Fenster zu öffnen,
//! - der spätere Flash-Helper (läuft unter Linux mit Root-Rechten) kann dieselben
//!   Sicherheitsprüfungen nutzen, ohne die ganze GUI mitzuschleppen.

pub mod acquire;
pub mod catalog;
pub mod device;
pub mod download;
pub mod flash;
pub mod http;
pub mod resolve;
pub mod verify;
