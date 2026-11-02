//! # sealflash-core
//!
//! Die gesamte Logik von SealFlash – bewusst **ohne** Abhängigkeit zu Tauri/GUI.
//! Vorteile:
//! - mit `cargo test` testbar, ohne ein Fenster zu öffnen,
//! - der spätere Flash-Helper (läuft unter Linux mit Root-Rechten) kann dieselben
//!   Sicherheitsprüfungen nutzen, ohne die ganze GUI mitzuschleppen.

pub mod acquire;
pub mod catalog;
pub mod device;
pub mod disk;
pub mod download;
pub mod flash;
pub mod http;
pub mod local;
pub mod resolve;
pub mod verify;
