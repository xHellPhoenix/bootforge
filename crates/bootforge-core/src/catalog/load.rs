//! Katalog laden: Online → Cache → eingebaut.
//!
//! ```text
//!   Online (HTTPS, mit ETag)
//!     ├─ 304 Not Modified ─────────────► Cache verwenden (ist aktuell)
//!     ├─ 200 + gültig ─────────────────► verwenden + Cache überschreiben
//!     └─ Netzwerkfehler / ungültig ────► Cache (falls gültig) ──► sonst eingebauter Katalog
//! ```
//!
//! Der Cache wird **beim Lesen erneut validiert**: Er könnte beschädigt sein oder von einer
//! älteren App-Version stammen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use super::{Catalog, Issue, bundled, parse};
use crate::http::{Conditional, Fetcher};

/// Offizielle Katalog-URL.
pub const DEFAULT_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/xHellPhoenix/bootforge-catalog/main/catalog.json";

const CACHE_FILE: &str = "catalog.json";
const META_FILE: &str = "catalog.meta.json";

/// Woher der verwendete Katalog stammt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Gerade online geladen bzw. vom Server als aktuell bestätigt.
    Remote,
    /// Online nicht erreichbar/ungültig → zuletzt gespeicherte Version.
    Cache,
    /// Weder online noch Cache → in die App eingebaute Version.
    Bundled,
}

/// Ergebnis von [`load`].
#[derive(Debug, Clone, Serialize)]
pub struct Loaded {
    pub catalog: Catalog,
    pub origin: Origin,
    pub warnings: Vec<Issue>,
    /// Warum nicht der Online-Katalog verwendet wird (für den Hinweis in der GUI).
    pub fallback_reason: Option<String>,
    /// Der Online-Katalog war erreichbar, passt aber nicht zu dieser App-Version (andere
    /// Felder/Typen). Dann hilft nur ein App-Update – die GUI sagt das so.
    pub remote_rejected: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Meta {
    etag: Option<String>,
}

/// Katalog laden – schlägt nie fehl, im schlimmsten Fall kommt der eingebaute Katalog.
pub async fn load<F: Fetcher>(f: &F, url: &Url, cache_dir: &Path) -> Loaded {
    let cache = read_cache(cache_dir).await;
    // ETag nur mitschicken, wenn wir auch einen gültigen Cache haben, auf den wir zurückgreifen können.
    let etag = cache.as_ref().and_then(|(_, _, meta)| meta.etag.clone());

    let mut remote_rejected = false;
    let reason = match f.get(url, etag.as_deref()).await {
        Ok(Conditional::NotModified) => {
            let (catalog, warnings, _) = cache.expect("ETag nur bei vorhandenem Cache gesendet");
            return Loaded {
                catalog,
                origin: Origin::Remote,
                warnings,
                fallback_reason: None,
                remote_rejected: false,
            };
        }
        Ok(Conditional::Body { bytes, etag }) => match parse(&String::from_utf8_lossy(&bytes)) {
            Ok((catalog, warnings)) => {
                // Cache-Fehler sind nicht schlimm (z. B. schreibgeschützt) – nur weitermachen.
                let _ = write_cache(cache_dir, &bytes, &Meta { etag }).await;
                return Loaded {
                    catalog,
                    origin: Origin::Remote,
                    warnings,
                    fallback_reason: None,
                    remote_rejected: false,
                };
            }
            Err(e) => {
                remote_rejected = true;
                format!("Online-Katalog abgelehnt: {e}")
            }
        },
        Err(e) => format!("Online-Katalog nicht erreichbar: {e}"),
    };

    if let Some((catalog, warnings, _)) = cache {
        return Loaded {
            catalog,
            origin: Origin::Cache,
            warnings,
            fallback_reason: Some(reason),
            remote_rejected,
        };
    }
    let (catalog, warnings) = bundled().expect("eingebauter Katalog wird durch Tests geprüft");
    Loaded {
        catalog,
        origin: Origin::Bundled,
        warnings,
        fallback_reason: Some(reason),
        remote_rejected,
    }
}

fn paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join(CACHE_FILE), dir.join(META_FILE))
}

async fn read_cache(dir: &Path) -> Option<(Catalog, Vec<Issue>, Meta)> {
    let (file, meta) = paths(dir);
    let text = tokio::fs::read_to_string(&file).await.ok()?;
    let (catalog, warnings) = parse(&text).ok()?;
    let meta = tokio::fs::read_to_string(&meta)
        .await
        .ok()
        .and_then(|m| serde_json::from_str(&m).ok())
        .unwrap_or_default();
    Some((catalog, warnings, meta))
}

/// Atomar schreiben: erst in eine temporäre Datei, dann umbenennen. Stürzt die App mitten im
/// Schreiben ab, bleibt so die alte, vollständige Datei erhalten statt einer halben.
async fn write_cache(dir: &Path, text: &[u8], meta: &Meta) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let (file, meta_file) = paths(dir);
    let tmp = dir.join("catalog.json.tmp");
    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, &file).await?;
    let meta_json = serde_json::to_string(meta).expect("Meta ist serialisierbar");
    tokio::fs::write(&meta_file, meta_json).await
}
