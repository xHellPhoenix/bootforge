//! Ein geprüftes Image auf einen USB-Stick schreiben.
//!
//! [`flash_drive`] ist der komplette, abgesicherte Ablauf. Er läuft
//! - unter **Windows** direkt in der App (die mit Administratorrechten läuft),
//! - unter **Linux** im kleinen Root-Helper (`bootforge-flash-helper`, gestartet über pkexec).
//!
//! In beiden Fällen prüft dieser Code das Laufwerk **selbst** noch einmal – er verlässt sich
//! nicht darauf, was die GUI geschickt hat.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::device::{self, Drive, ScanOptions};

pub mod blockcache;
pub mod fat32;
pub mod gpt;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;
pub mod windows_media;
pub mod write;

pub use write::{FlashError, FlashEvent, FlashJob, FlashReport, FlashStage};

/// Alles, was zum Schreiben nötig ist. Unter Linux geht das als JSON an den Helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForgeRequest {
    /// Das Laufwerk, wie es der Nutzer ausgewählt und bestätigt hat.
    pub drive: Drive,
    /// Mit welchen Optionen es ausgewählt wurde (externe Festplatten sichtbar?).
    pub opts: ScanOptions,
    pub job: FlashJob,
}

/// Der komplette Ablauf: Laufwerk neu prüfen → Image prüfen → vorbereiten → schreiben → zurücklesen.
pub fn flash_drive(
    req: &ForgeRequest,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    on_event(FlashEvent::Stage {
        stage: FlashStage::CheckDrive,
    });
    let drive = device::recheck(&req.drive, req.opts).map_err(FlashError::Drive)?;

    write::verify_image(&req.job, cancel, on_event)?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        // Noch nichts geschrieben → der Stick ist unverändert.
        return Err(FlashError::Cancelled);
    }

    on_event(FlashEvent::Stage {
        stage: FlashStage::Prepare,
    });
    platform_flash(&drive, req, cancel, on_event)
}

#[cfg(target_os = "linux")]
fn platform_flash(
    drive: &Drive,
    req: &ForgeRequest,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    let mut dev = linux::open(drive)?;
    let r = write::write_and_verify(&mut dev, drive.size, &req.job, cancel, on_event);
    linux::finish(dev);
    r
}

#[cfg(windows)]
fn platform_flash(
    drive: &Drive,
    req: &ForgeRequest,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    let mut dev = windows::open(drive)?;
    let r = write::write_and_verify(&mut dev, drive.size, &req.job, cancel, on_event);
    windows::finish(dev);
    r
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_flash(
    _drive: &Drive,
    _req: &ForgeRequest,
    _cancel: &AtomicBool,
    _on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    Err(FlashError::Unsupported("Betriebssystem".into()))
}

/// Nachrichten des Linux-Helpers an die App – eine JSON-Zeile pro Nachricht auf stdout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HelperMessage {
    Event { event: FlashEvent },
    Done { report: FlashReport },
    Error { error: FlashError },
}
