//! Ermittelt für jede Variante des Katalogs die neueste Version – gegen die echten Server.
//! Praktisch für alle, die den Katalog pflegen: zeigt sofort, welcher Eintrag kaputt ist.
//!
//!     cargo run -p bootforge-core --example resolve_all [pfad/zu/catalog.json]
//!
//! Exit-Code 1, wenn mindestens eine Variante fehlschlägt (für CI geeignet).

use bootforge_core::catalog;
use bootforge_core::http::HttpClient;
use bootforge_core::resolve::{ResolveError, resolve};

#[tokio::main]
async fn main() {
    let json = match std::env::args().nth(1) {
        Some(path) => std::fs::read_to_string(&path).expect("Katalogdatei lesbar"),
        None => catalog::BUNDLED_CATALOG_JSON.to_owned(),
    };
    let (cat, _) = catalog::parse(&json).unwrap_or_else(|e| panic!("{e}"));
    let http = HttpClient::new().expect("HTTP-Client");

    // Alle Varianten parallel auflösen (sind nur kleine Textabrufe).
    let jobs = cat.distros.iter().flat_map(|d| &d.variants).map(|v| {
        let http = http.clone();
        let v = v.clone(); // eigene Kopie: der Task darf nichts von `cat` ausleihen
        async move { (v.id.clone(), resolve(&http, &v).await) }
    });
    let results = futures_join_all(jobs).await;

    let mut failed = 0;
    for (id, r) in results {
        match r {
            Ok(img) => {
                let size = img
                    .size
                    .map(|s| format!("{:.2} GB", s as f64 / 1e9))
                    .unwrap_or_else(|| "?".into());
                println!("✔ {id:<24} {:<12} {size:>9}  {}", img.version, img.url);
            }
            Err(ResolveError::NotSupportedYet(what)) => {
                println!("· {id:<24} ({what}: folgt später)")
            }
            Err(e) => {
                failed += 1;
                println!("✘ {id:<24} {e}");
            }
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Mini-Ersatz für `futures::future::join_all`, um keine Abhängigkeit nur fürs Beispiel zu brauchen.
async fn futures_join_all<T: Send + 'static>(
    futs: impl Iterator<Item = impl std::future::Future<Output = T> + Send + 'static>,
) -> Vec<T> {
    let handles: Vec<_> = futs.map(tokio::spawn).collect();
    let mut out = Vec::new();
    for h in handles {
        out.push(h.await.expect("Task abgestürzt"));
    }
    out
}
