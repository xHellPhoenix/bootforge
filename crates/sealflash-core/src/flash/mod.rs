//! Ein geprüftes Image auf einen USB-Stick schreiben.
//!
//! [`flash_drive`] ist der komplette, abgesicherte Ablauf. Er läuft
//! - unter **Windows** direkt in der App (die mit Administratorrechten läuft),
//! - unter **Linux** im kleinen Root-Helper (`sealflash-flash-helper`, gestartet über pkexec).
//!
//! In beiden Fällen prüft dieser Code das Laufwerk **selbst** noch einmal – er verlässt sich
//! nicht darauf, was die GUI geschickt hat.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::catalog::{Compression, FlashMode};
use crate::device::{self, Drive, ScanOptions};

pub mod blockcache;
pub mod fat32;
pub mod gpt;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod mount;
pub mod unattend;
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
    /// Raw (1:1) oder Windows (GPT + FAT32). Fehlt das Feld, gilt Raw.
    #[serde(default = "raw")]
    pub mode: FlashMode,
    /// Nur Windows-Modus: Einstellungen für die Antwortdatei (`autounattend.xml`).
    #[serde(default)]
    pub windows_setup: Option<unattend::WindowsSetup>,
}

fn raw() -> FlashMode {
    FlashMode::Raw
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

    // Antwortdatei: nur im Windows-Modus, Eingaben prüfen, BEVOR irgendetwas passiert.
    if let Some(ws) = req.windows_setup.as_ref().filter(|w| !w.is_empty()) {
        if req.mode != FlashMode::Windows {
            return Err(FlashError::Unsupported(
                "Windows-Einstellungen gibt es nur für Windows-Installationsmedien".into(),
            ));
        }
        ws.validate().map_err(FlashError::Unsupported)?;
    }
    if req.mode == FlashMode::Windows && req.job.compression != Compression::None {
        return Err(FlashError::Unsupported(
            "Windows-Modus nur mit unkomprimierten ISO-Dateien".into(),
        ));
    }
    // Fingerabdruck VOR der Prüfung: Im Windows-Modus wird die Datei danach erneut geöffnet
    // (eingebunden) – sie muss dann noch dieselbe sein.
    let stamp = mount::FileStamp::of(&req.job.image)?;
    write::verify_image(&req.job, cancel, on_event)?;
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        // Noch nichts geschrieben → der Stick ist unverändert.
        return Err(FlashError::Cancelled);
    }

    on_event(FlashEvent::Stage {
        stage: FlashStage::Prepare,
    });
    platform_flash(&drive, req, &stamp, cancel, on_event)
}

#[cfg(target_os = "linux")]
use linux::{finish as finish_device, open as open_device};
#[cfg(windows)]
use windows::{finish as finish_device, open as open_device};

#[cfg(any(target_os = "linux", windows))]
fn platform_flash(
    drive: &Drive,
    req: &ForgeRequest,
    stamp: &mount::FileStamp,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    match req.mode {
        FlashMode::Raw => {
            let mut dev = open_device(drive)?;
            let r = write::write_and_verify(&mut dev, drive.size, &req.job, cancel, on_event);
            finish_device(dev);
            r
        }
        FlashMode::Windows => {
            // Reihenfolge: erst einbinden und Arbeitsverzeichnis anlegen (kann scheitern, ohne
            // dass der Stick angefasst wurde), dann erst das Gerät öffnen.
            let iso = mount::MountedIso::mount(&req.job.image, stamp)?;
            let work = mount::PrivateDir::new(&mount::work_base(), "sealflash-work")?;
            let mut dev = open_device(drive)?;
            let r = (|| {
                let sector = dev.logical_sector_size()?;
                if sector != 512 {
                    return Err(FlashError::Unsupported(format!(
                        "Laufwerk mit {sector}-Byte-Sektoren (nur 512 wird unterstützt)"
                    )));
                }
                let mut opts = windows_media::MediaOptions::new(work.path().to_owned());
                if let Some(ws) = req.windows_setup.as_ref().filter(|w| !w.is_empty()) {
                    let lang = read_lang_ini(iso.root());
                    let xml = ws
                        .to_xml(lang.as_deref())
                        .map_err(FlashError::Unsupported)?;
                    opts.extra_files
                        .push(("autounattend.xml".into(), xml.into_bytes()));
                }
                let report = windows_media::write_windows_media(
                    &mut dev,
                    drive.size,
                    iso.root(),
                    &opts,
                    cancel,
                    on_event,
                )?;
                // Nur was geprüft wurde, darf auf dem Stick bleiben.
                if let Err(e) = iso.check_unchanged() {
                    write::wipe_head(&mut dev);
                    return Err(e);
                }
                Ok(report)
            })();
            drop(iso);
            finish_device(dev);
            r
        }
    }
}

/// Sprache der ISO aus `sources/lang.ini` (Groß-/Kleinschreibung egal, unter Linux ist UDF
/// case-sensitive). Fehlt die Datei, wird keine Setup-Sprache erzwungen.
#[cfg(any(target_os = "linux", windows))]
fn read_lang_ini(root: &std::path::Path) -> Option<String> {
    let find = |dir: &std::path::Path, name: &str| {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            e.file_name()
                .to_str()
                .filter(|n| n.eq_ignore_ascii_case(name))
                .map(|_| e.path())
        })
    };
    let sources = find(root, "sources")?;
    let ini = find(&sources, "lang.ini")?;
    let bytes = std::fs::read(ini).ok()?;
    // lang.ini ist meist UTF-16 (mit BOM) – beides verstehen.
    let text = if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| u16::from_le_bytes(c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    unattend::ui_language_from_lang_ini(&text)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn platform_flash(
    _drive: &Drive,
    _req: &ForgeRequest,
    _stamp: &mount::FileStamp,
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

#[cfg(all(test, any(target_os = "linux", windows)))]
mod tests {
    #[test]
    fn reads_utf16_lang_ini_case_insensitive() {
        let root = std::env::temp_dir().join(format!("sf-langini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("SOURCES")).unwrap();
        // So liefert Microsoft die Datei: UTF-16 LE mit BOM, Windows-Zeilenenden.
        let text =
            "[Available UI Languages]\r\nde-DE = 3\r\n[Fallback Languages]\r\nde-DE = en-us\r\n";
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(root.join("SOURCES/Lang.ini"), bytes).unwrap();
        assert_eq!(super::read_lang_ini(&root).as_deref(), Some("de-DE"));
        // Ohne Datei: keine Sprache erzwingen.
        std::fs::remove_file(root.join("SOURCES/Lang.ini")).unwrap();
        assert_eq!(super::read_lang_ini(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
