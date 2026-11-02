//! „Flash!“ ohne GUI: neueste Version laden, prüfen und auf einen USB-Stick schreiben.
//!
//! **Löscht alle Daten auf dem gewählten Stick!**
//!
//! Windows (PowerShell **als Administrator**):
//!     cargo run --release -p sealflash-core --example forge -- debian-netinst 1
//! Linux:
//!     cargo build --release -p sealflash-core --example forge
//!     sudo ./target/release/examples/forge debian-netinst /dev/sdb
//!
//! Das zweite Argument ist das Laufwerk: unter Windows die Nummer (aus `list_drives`,
//! „PhysicalDrive1“ → `1`), unter Linux der Gerätepfad.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sealflash_core::acquire::{AcquireEvent, acquire};
use sealflash_core::device::{ScanOptions, list};
use sealflash_core::flash::{FlashEvent, FlashJob, ForgeRequest, flash_drive};
use sealflash_core::http::HttpClient;
use sealflash_core::{catalog, resolve};

fn progress(stage: &str, done: u64, total: Option<u64>, bps: f64) {
    let pct = total
        .map(|t| format!("{:5.1} %", done as f64 * 100.0 / t.max(1) as f64))
        .unwrap_or_default();
    print!(
        "\r  {stage:<12} {pct}  {:>9.1} MB  {:>6.1} MB/s   ",
        done as f64 / 1e6,
        bps / 1e6
    );
    let _ = std::io::stdout().flush();
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [variant_id, target] = args.as_slice() else {
        eprintln!("Aufruf: forge <varianten-id> <laufwerk>   (z. B. forge debian-netinst 1)");
        std::process::exit(2);
    };
    let drive_id = if cfg!(windows) && target.chars().all(|c| c.is_ascii_digit()) {
        format!(r"\\.\PhysicalDrive{target}")
    } else {
        target.clone()
    };

    // 1) Laufwerk finden – nur wählbare Laufwerke kommen in Frage.
    let drives = list(ScanOptions::default()).expect("Laufwerke lesbar");
    let Some(drive) = drives.iter().find(|d| d.id == drive_id).cloned() else {
        eprintln!("✘ {drive_id} nicht gefunden. Vorhandene Laufwerke:");
        for d in &drives {
            eprintln!("    {} {} ({:.1} GB)", d.id, d.name, d.size as f64 / 1e9);
        }
        std::process::exit(1);
    };
    if let Some(r) = &drive.rejection {
        eprintln!(
            "✘ {} ({}) darf nicht beschrieben werden: {r:?}",
            drive.id, drive.name
        );
        std::process::exit(1);
    }

    // 2) Version ermitteln
    let (cat, _) = catalog::bundled().unwrap();
    let Some(variant) = cat
        .distros
        .iter()
        .flat_map(|d| &d.variants)
        .find(|v| &v.id == variant_id)
    else {
        eprintln!("✘ unbekannte Variante {variant_id}");
        std::process::exit(1);
    };
    let http = HttpClient::new().unwrap();
    let img = resolve::resolve(&http, variant)
        .await
        .unwrap_or_else(|e| panic!("{e}"));

    // 3) Bestätigung – wie in der App: Namen abtippen.
    println!();
    println!("  ⚠  ACHTUNG: ALLE DATEN auf diesem Laufwerk werden gelöscht:");
    println!(
        "       {}  –  {}  –  {:.1} GB",
        drive.id,
        drive.name,
        drive.size as f64 / 1e9
    );
    if !drive.mounts.is_empty() {
        println!("       eingebunden: {}", drive.mounts.join(", "));
    }
    println!(
        "     Geschrieben wird: {} {} ({})",
        variant.id, img.version, img.filename
    );
    print!(
        "\n  Zum Bestätigen den Namen des Laufwerks eintippen ({}): ",
        drive.name
    );
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).unwrap();
    if !answer.trim().eq_ignore_ascii_case(drive.name.trim()) {
        println!("  Abgebrochen – nichts wurde verändert.");
        std::process::exit(1);
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        c.store(true, Ordering::SeqCst);
    });

    // 4) Beschaffen + prüfen
    let dir = std::env::temp_dir().join("sealflash-images");
    println!("\n▶ Image beschaffen und prüfen ({})", dir.display());
    let acquired = match acquire(&http, &img, &dir, &cancel, |e| match e {
        AcquireEvent::Stage { stage } => println!("\n  · {stage:?}"),
        AcquireEvent::Signed { signer } => println!(
            "    ✔ signiert von {}",
            signer.user_id.unwrap_or(signer.fingerprint)
        ),
        AcquireEvent::Progress {
            stage,
            done,
            total,
            bytes_per_sec,
        } => progress(&format!("{stage:?}"), done, total, bytes_per_sec),
    })
    .await
    {
        Ok(a) => a,
        Err(e) => {
            println!("\n✘ {e}");
            std::process::exit(1);
        }
    };
    println!(
        "\n  ✔ {:?} – {} {}",
        acquired.assurance,
        acquired.algorithm_name(),
        acquired.hash
    );

    // 5) Schreiben (blockierend → eigener Thread)
    println!("\n▶ Auf {} schreiben", drive.id);
    let req = ForgeRequest {
        drive,
        opts: ScanOptions::default(),
        job: FlashJob {
            image: acquired.path.clone(),
            compression: img.compression,
            algorithm: acquired.algorithm,
            expected_hash: acquired.hash.clone(),
        },
        mode: img.flash_mode,
        windows_setup: None,
    };
    let started = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        flash_drive(&req, &cancel, &mut |e| match e {
            FlashEvent::Stage { stage } => println!("\n  · {stage:?}"),
            FlashEvent::Progress {
                stage,
                done,
                total,
                bytes_per_sec,
            } => progress(&format!("{stage:?}"), done, total, bytes_per_sec),
        })
    })
    .await
    .unwrap();

    match result {
        Ok(r) => println!(
            "\n\n✔ Fertig in {:.0?}: {:.1} MB geschrieben und durch Zurücklesen bestätigt (SHA-256 {}).\n  Der Stick kann abgezogen werden.",
            started.elapsed(),
            r.written as f64 / 1e6,
            r.sha256
        ),
        Err(e) => {
            println!("\n\n✘ {e}");
            std::process::exit(1);
        }
    }
}
