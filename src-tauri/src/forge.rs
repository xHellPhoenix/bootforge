//! „Flash!“: Image beschaffen und auf den Stick schreiben.
//!
//! - Windows: Die App läuft (im Release) als Administrator und schreibt selbst.
//! - Linux: Die App läuft als normaler Nutzer und startet den Root-Helper über pkexec.
//!   Er bekommt die Anfrage als JSON über stdin und meldet Fortschritt als JSON-Zeilen.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sealflash_core::flash::{FlashEvent, FlashReport, FlashStage, ForgeRequest};
use tauri::ipc::Channel;

use crate::error::AppError;
use crate::events::ForgeEvent;

/// Schreibt über die passende Plattform-Variante. `wrote` wird gesetzt, sobald das
/// Schreiben begonnen hat (für die richtige Abbruch-Meldung).
pub async fn flash(
    req: ForgeRequest,
    cancel: Arc<AtomicBool>,
    events: Channel<ForgeEvent>,
) -> Result<FlashReport, AppError> {
    platform_flash(req, cancel, events).await
}

/// Ab diesen Schritten wird auf den Stick geschrieben (Raw: Write; Windows: ab Partition).
fn note_stage(e: &FlashEvent, wrote: &AtomicBool) {
    if let FlashEvent::Stage {
        stage: FlashStage::Write | FlashStage::Partition | FlashStage::Format | FlashStage::Copy,
    } = e
    {
        wrote.store(true, Ordering::SeqCst);
    }
}

#[cfg(not(target_os = "linux"))]
async fn platform_flash(
    req: ForgeRequest,
    cancel: Arc<AtomicBool>,
    events: Channel<ForgeEvent>,
) -> Result<FlashReport, AppError> {
    let wrote = Arc::new(AtomicBool::new(false));
    let w = wrote.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        sealflash_core::flash::flash_drive(&req, &cancel, &mut |e| {
            note_stage(&e, &w);
            let _ = events.send(ForgeEvent::Flash(e));
        })
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    result.map_err(|e| AppError::from_flash(e, wrote.load(Ordering::SeqCst)))
}

#[cfg(target_os = "linux")]
async fn platform_flash(
    req: ForgeRequest,
    cancel: Arc<AtomicBool>,
    events: Channel<ForgeEvent>,
) -> Result<FlashReport, AppError> {
    use std::process::Stdio;
    use std::time::Duration;

    use sealflash_core::flash::HelperMessage;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let helper = helper_path().ok_or_else(|| {
        AppError::HelperMissing(
            "/usr/lib/sealflash/sealflash-flash-helper fehlt (SealFlash über das .deb/.rpm-Paket installieren)".into(),
        )
    })?;

    // Schon root (z. B. im Container)? Dann direkt, sonst über pkexec.
    // SAFETY: geteuid hat keine Vorbedingungen.
    let mut cmd = if unsafe { libc::geteuid() } == 0 {
        tokio::process::Command::new(&helper)
    } else {
        let mut c = tokio::process::Command::new("pkexec");
        c.arg(&helper);
        c
    };
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            AppError::HelperMissing(format!("pkexec/Helper lässt sich nicht starten: {e}"))
        })?;

    let mut stdin = child.stdin.take().expect("piped");
    let line = serde_json::to_string(&req).map_err(|e| AppError::Internal(e.to_string()))?;
    stdin
        .write_all(format!("{line}\n").as_bytes())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    let wrote = AtomicBool::new(false);
    let mut cancel_sent = false;
    let mut outcome: Option<Result<FlashReport, AppError>> = None;

    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => match serde_json::from_str::<HelperMessage>(&l) {
                    Ok(HelperMessage::Event { event }) => {
                        note_stage(&event, &wrote);
                        let _ = events.send(ForgeEvent::Flash(event));
                    }
                    Ok(HelperMessage::Done { report }) => outcome = Some(Ok(report)),
                    Ok(HelperMessage::Error { error }) => {
                        outcome = Some(Err(AppError::from_flash(error, wrote.load(Ordering::SeqCst))))
                    }
                    Err(_) => {} // unbekannte Zeile ignorieren
                },
                _ => break, // Helper beendet
            },
            _ = tick.tick() => {
                if cancel.load(Ordering::SeqCst) && !cancel_sent {
                    let _ = stdin.write_all(b"cancel\n").await;
                    cancel_sent = true;
                }
            }
        }
    }
    drop(stdin);
    let status = child
        .wait()
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    match outcome {
        Some(r) => r,
        // pkexec: 126 = Dialog abgebrochen, 127 = nicht berechtigt
        None if matches!(status.code(), Some(126 | 127)) => Err(AppError::AuthCancelled),
        None => Err(AppError::Internal(format!(
            "Helper ohne Ergebnis beendet ({status})"
        ))),
    }
}

/// Wo liegt der Helper?
///
/// Im Release-Build **nur** systemweit (`/usr/lib/sealflash`, aus dem .deb/.rpm). Dorthin kann nur
/// root schreiben, und genau diesen Pfad nennt die polkit-Regel. Ein Helper „neben der App“ wird
/// nur im Debug-Build gesucht (Entwicklung mit `cargo build`).
#[cfg(target_os = "linux")]
fn helper_path() -> Option<std::path::PathBuf> {
    let name = "sealflash-flash-helper";
    let system = std::path::PathBuf::from("/usr/lib/sealflash").join(name);
    #[cfg(debug_assertions)]
    {
        let next_to_app = std::env::current_exe().ok()?.parent()?.join(name);
        if next_to_app.is_file() {
            return Some(next_to_app);
        }
    }
    system.is_file().then_some(system)
}
