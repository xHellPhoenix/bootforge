//! Lädt eine Variante herunter und prüft sie vollständig – ohne GUI.
//!
//!     cargo run --release -p sealflash-core --example acquire_one -- debian-netinst /tmp/isos
//!
//! Strg+C bricht ab; ein erneuter Aufruf setzt den Download fort.

use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sealflash_core::acquire::{AcquireEvent, acquire};
use sealflash_core::catalog;
use sealflash_core::http::HttpClient;
use sealflash_core::resolve::resolve;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let id = args
        .next()
        .expect("Varianten-ID angeben, z. B. debian-netinst");
    let dir = std::path::PathBuf::from(args.next().unwrap_or_else(|| "isos".into()));

    let (cat, _) = catalog::bundled().unwrap();
    let variant = cat
        .distros
        .iter()
        .flat_map(|d| &d.variants)
        .find(|v| v.id == id)
        .expect("unbekannte Variante");
    let http = HttpClient::new().unwrap();
    let img = resolve(&http, variant)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    println!("{} {} → {}", img.variant_id, img.version, img.url);

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        c.store(true, Ordering::SeqCst);
    });

    let started = std::time::Instant::now();
    let r = acquire(&http, &img, &dir, &cancel, |e| match e {
        AcquireEvent::Stage { stage } => println!("\n▶ {stage:?}"),
        AcquireEvent::Signed { signer } => println!(
            "  ✔ signiert von {} ({})",
            signer.user_id.unwrap_or_default(),
            signer.fingerprint
        ),
        AcquireEvent::Progress {
            done,
            total,
            bytes_per_sec,
            ..
        } => {
            let pct = total
                .map(|t| format!("{:5.1} %", done as f64 * 100.0 / t as f64))
                .unwrap_or_default();
            print!(
                "\r  {pct}  {:>8.1} MB  {:>6.1} MB/s ",
                done as f64 / 1e6,
                bytes_per_sec / 1e6
            );
            let _ = std::io::stdout().flush();
        }
    })
    .await;
    match r {
        Ok(a) => println!(
            "\n✔ fertig in {:.0?}: {} ({:?}, {:?} {}, aus Cache: {})",
            started.elapsed(),
            a.path.display(),
            a.assurance,
            a.algorithm,
            a.hash,
            a.from_cache
        ),
        Err(e) => {
            println!("\n✘ {e}");
            std::process::exit(1);
        }
    }
}
