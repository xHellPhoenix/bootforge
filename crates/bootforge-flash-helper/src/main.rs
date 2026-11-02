//! # bootforge-flash-helper
//!
//! Läuft unter Linux mit Root-Rechten (gestartet über `pkexec`) und macht nur eins:
//! ein geprüftes Image auf einen USB-Stick schreiben.
//!
//! Warum ein eigenes Programm? Die GUI soll NICHT als root laufen: ein Browser-Engine-Prozess
//! (WebKitGTK) mit Root-Rechten wäre eine riesige Angriffsfläche. Dieser Helper ist klein,
//! hat keine GUI und kein Netzwerk.
//!
//! Protokoll:
//! - stdin, 1. Zeile: `ForgeRequest` als JSON
//! - stdin danach: `cancel` → abbrechen. **Schließt die App stdin (z. B. Absturz), wird ebenfalls
//!   abgebrochen** – ein Root-Prozess soll nicht verwaist weiterschreiben.
//! - stdout: eine JSON-Zeile pro `HelperMessage`
//!
//! Der Helper vertraut der Anfrage nicht: Das Laufwerk wird neu erkannt und muss alle
//! Sicherheitsregeln erfüllen, das Image muss dem aufrufenden Nutzer gehören und zur
//! Prüfsumme passen.

use std::process::ExitCode;

/// Die eigentliche Arbeit – nur unter Linux (unter Windows schreibt die App selbst).
#[cfg(target_os = "linux")]
mod imp {
    use super::ExitCode;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use bootforge_core::flash::{FlashError, ForgeRequest, HelperMessage};

    fn send(msg: &HelperMessage) {
        let mut out = std::io::stdout().lock();
        let _ = serde_json::to_writer(&mut out, msg);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }

    fn fail(error: FlashError) -> ExitCode {
        send(&HelperMessage::Error { error });
        ExitCode::FAILURE
    }

    /// Das Image muss eine normale Datei sein (kein Symlink, kein Gerät) und – wenn über pkexec
    /// gestartet – dem aufrufenden Nutzer gehören. Sonst könnte jemand den Root-Helper benutzen,
    /// um fremde Dateien (z. B. /etc/shadow) auf einen Stick zu kopieren.
    fn check_image_owner(req: &ForgeRequest) -> Result<(), FlashError> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::symlink_metadata(&req.job.image)
            .map_err(|e| FlashError::Io(format!("{}: {e}", req.job.image.display())))?;
        if !meta.file_type().is_file() {
            return Err(FlashError::Io("Image ist keine normale Datei".into()));
        }
        if let Ok(uid) = std::env::var("PKEXEC_UID") {
            let uid: u32 = uid
                .parse()
                .map_err(|_| FlashError::Io("PKEXEC_UID ungültig".into()))?;
            if meta.uid() != uid {
                return Err(FlashError::Io(
                    "Image gehört nicht dem aufrufenden Benutzer".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn main() -> ExitCode {
        let mut first = String::new();
        if std::io::stdin().read_line(&mut first).unwrap_or(0) == 0 {
            return fail(FlashError::Io("keine Anfrage auf stdin".into()));
        }
        let req: ForgeRequest = match serde_json::from_str(&first) {
            Ok(r) => r,
            Err(e) => return fail(FlashError::Io(format!("Anfrage unlesbar: {e}"))),
        };
        if let Err(e) = check_image_owner(&req) {
            return fail(e);
        }

        // Abbruch-Wächter: „cancel“ oder Ende von stdin.
        let cancel = Arc::new(AtomicBool::new(false));
        let c = cancel.clone();
        // Der stdin-Zugriff wird IM Thread geholt: ein `StdinLock` darf nicht zwischen Threads wandern.
        std::thread::spawn(move || {
            for line in std::io::stdin().lines() {
                match line {
                    Ok(l) if l.trim() == "cancel" => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            c.store(true, Ordering::SeqCst);
        });

        match bootforge_core::flash::flash_drive(&req, &cancel, &mut |event| {
            send(&HelperMessage::Event { event })
        }) {
            Ok(report) => {
                send(&HelperMessage::Done { report });
                ExitCode::SUCCESS
            }
            Err(e) => fail(e),
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    imp::main()
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("bootforge-flash-helper wird nur unter Linux benötigt.");
    ExitCode::FAILURE
}
