//! Zeigt alle Laufwerke mit Urteil – zum Ausprobieren der Erkennung ohne GUI.
//!
//!     cargo run -p bootforge-core --example list_drives [--hdd]
use bootforge_core::device::{ScanOptions, list};

fn main() {
    let opts = ScanOptions {
        show_external_hdds: std::env::args().any(|a| a == "--hdd"),
    };
    for d in list(opts).expect("Laufwerke lesbar") {
        let verdict = match &d.rejection {
            None => "✔ WÄHLBAR".to_string(),
            Some(r) => format!("✘ {r:?}"),
        };
        println!(
            "{:<14} {:>9.1} GB  {:<28} {verdict}",
            d.id,
            d.size as f64 / 1e9,
            d.name
        );
    }
}
