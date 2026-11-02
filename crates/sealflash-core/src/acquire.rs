//! Ein Image beschaffen: Prüfsumme holen → (Signatur prüfen) → herunterladen → prüfen.
//!
//! Die Reihenfolge ist Absicht:
//!
//! ```text
//!  1. Prüfsummendatei laden + GPG-Signatur prüfen   ← Sekunden; schlägt hier etwas fehl,
//!                                                     wird gar nicht erst heruntergeladen
//!  2. vorhandene Datei im Cache?  → neu hashen, bei Treffer kein Download
//!  3. herunterladen (Hash läuft mit)
//!  4. Hash vergleichen  → bei Abweichung Datei LÖSCHEN
//!  5. ggf. Signatur über das ganze Image (Arch, Proxmox)
//! ```
//!
//! Erst wenn alles stimmt, gibt es ein [`Acquired`]. Nur damit darf in Phase 5 geflasht werden.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::catalog::{HashAlgorithm, SignatureKind, SignedTarget};
use crate::download::{DownloadError, Progress, RangeSource, download_pinned, hash_file};
use crate::http::{FetchError, Fetcher};
use crate::resolve::{ResolvedChecksum, ResolvedImage};
use crate::verify::gpg::{self, GpgError, Signer};
use crate::verify::sums::{Hasher, find_hash, hashes_equal, strip_clearsign_frame};

/// Abschnitte des Ablaufs (für die Fortschrittsanzeige).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Prüfsummendatei laden.
    Checksum,
    /// Signatur der Prüfsummendatei prüfen.
    Signature,
    /// Vorhandene Datei im Cache prüfen.
    CacheCheck,
    Download,
    /// Hash vergleichen.
    Verify,
    /// Signatur über das ganze Image prüfen.
    ImageSignature,
}

/// Ereignisse an die GUI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AcquireEvent {
    Stage {
        stage: Stage,
    },
    Progress {
        stage: Stage,
        done: u64,
        total: Option<u64>,
        bytes_per_sec: f64,
    },
    Signed {
        signer: Signer,
    },
}

/// Wie gut ist das Image abgesichert?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Assurance {
    /// Prüfsumme + GPG-Signatur des Herstellers.
    Signed,
    /// Nur Prüfsumme (SHA-256/512) über HTTPS.
    Checksum,
    /// Nur MD5 – schützt nur gegen Übertragungsfehler.
    Weak,
    /// Keine Prüfsumme vom Hersteller: per HTTPS direkt vom offiziellen Server geladen.
    /// Schützt vor Manipulation unterwegs, nicht vor einer ausgetauschten Datei auf dem Server.
    Origin,
}

/// Ein vollständig geprüftes Image.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Acquired {
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    pub algorithm: HashAlgorithm,
    pub hash: String,
    pub signer: Option<Signer>,
    pub assurance: Assurance,
    /// `true`, wenn kein Download nötig war.
    pub from_cache: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AcquireError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error(transparent)]
    Download(DownloadError),
    #[error(transparent)]
    Gpg(#[from] GpgError),
    #[error("die Prüfsummendatei {url} enthält keinen Eintrag für {filename}")]
    ChecksumNotListed { filename: String, url: String },
    #[error(
        "Prüfsumme stimmt NICHT – erwartet {expected}, berechnet {actual}. Die Datei wurde gelöscht."
    )]
    ChecksumMismatch { expected: String, actual: String },
    #[error("für die Signaturprüfung ist keine Schlüsselquelle (keyring_url) im Katalog angegeben")]
    MissingKeyring,
    #[error("unzulässiger Dateiname `{0}`")]
    BadFilename(String),
    #[error("abgebrochen")]
    Cancelled,
}

impl Acquired {
    /// „SHA-256“, „SHA-512“, „MD5“ – für Anzeigen.
    pub fn algorithm_name(&self) -> &'static str {
        match self.algorithm {
            HashAlgorithm::Sha256 => "SHA-256",
            HashAlgorithm::Sha512 => "SHA-512",
            HashAlgorithm::Md5 => "MD5",
        }
    }
}

impl From<DownloadError> for AcquireError {
    fn from(e: DownloadError) -> Self {
        match e {
            DownloadError::Cancelled => AcquireError::Cancelled,
            e => AcquireError::Download(e),
        }
    }
}

/// Dateinamen kommen vom Server – also misstrauen. Kein Pfad, keine Steuer- oder unter Windows
/// verbotenen Zeichen, nichts Verstecktes. Sonst könnte ein Server z. B. `../../.bashrc` liefern.
pub fn safe_filename(name: &str) -> Result<&str, AcquireError> {
    let bad = name.is_empty()
        || name.len() > 200
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c));
    if bad {
        Err(AcquireError::BadFilename(name.into()))
    } else {
        Ok(name)
    }
}

/// Beschafft und prüft ein Image. `dir` ist das Download-Verzeichnis.
pub async fn acquire<S: Fetcher + RangeSource>(
    src: &S,
    img: &ResolvedImage,
    dir: &Path,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(AcquireEvent) + Send,
) -> Result<Acquired, AcquireError> {
    let filename = safe_filename(&img.filename)?.to_owned();
    let dest = dir.join(&filename);
    let algorithm = img.checksum.algorithm();
    let check_cancel = || {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            Err(AcquireError::Cancelled)
        } else {
            Ok(())
        }
    };

    // Hersteller ohne Prüfsumme: nur Herkunft prüfbar (siehe `Checksum::HttpsOrigin`).
    let origin_hosts = match &img.checksum {
        ResolvedChecksum::HttpsOrigin { allowed_hosts } => Some(allowed_hosts.clone()),
        _ => None,
    };

    // --- 1. erwartete Prüfsumme (+ Signatur der Prüfsummendatei) --------------------------------
    if origin_hosts.is_none() {
        on_event(AcquireEvent::Stage {
            stage: Stage::Checksum,
        });
    }
    let mut signer = None;
    let mut certs = Vec::new();
    if let Some(sig) = &img.signature {
        let keyring_url = sig
            .keyring_url
            .as_ref()
            .ok_or(AcquireError::MissingKeyring)?;
        let keyring = src.get_bytes(keyring_url).await?;
        certs = gpg::pinned_certs(&keyring, &sig.fingerprints)?;
    }

    let expected: Option<String> = match &img.checksum {
        ResolvedChecksum::HttpsOrigin { .. } => None,
        ResolvedChecksum::Value { value, .. } => Some(value.clone()),
        ResolvedChecksum::File { url, .. } => Some({
            // Bytes, nicht Text: die Signatur gilt für die exakten Bytes der Datei.
            let sums = src.get_bytes(url).await?;
            let signed_text = match &img.signature {
                Some(sig) if sig.signs == SignedTarget::ChecksumFile => {
                    on_event(AcquireEvent::Stage {
                        stage: Stage::Signature,
                    });
                    let (text, who) = match (sig.kind, &sig.url) {
                        (SignatureKind::GpgClearsigned, _) => {
                            gpg::verify_cleartext(&sums, &certs, &sig.fingerprints, sig.allow_v3)?
                        }
                        (SignatureKind::GpgDetached, Some(sig_url)) => {
                            let signature = src.get_bytes(sig_url).await?;
                            let who = gpg::verify_detached(
                                &sums[..],
                                &signature,
                                &certs,
                                &sig.fingerprints,
                                sig.allow_v3,
                            )?;
                            (String::from_utf8_lossy(&sums).into_owned(), who)
                        }
                        (SignatureKind::GpgDetached, None) => {
                            unreachable!("vom Validator ausgeschlossen")
                        }
                    };
                    on_event(AcquireEvent::Signed {
                        signer: who.clone(),
                    });
                    signer = Some(who);
                    // Nur den VERIFIZIERTEN Text weiterverwenden.
                    text
                }
                _ => strip_clearsign_frame(&String::from_utf8_lossy(&sums)).into_owned(),
            };
            find_hash(&signed_text, &filename, algorithm).ok_or_else(|| {
                AcquireError::ChecksumNotListed {
                    filename: filename.clone(),
                    url: url.to_string(),
                }
            })?
        }),
    };
    // Ohne Hersteller-Prüfsumme halten wir den beim Download berechneten Hash neben der Datei
    // fest. Beim nächsten Mal fällt so wenigstens eine beschädigte Datei im Cache auf.
    let sidecar = dest.with_file_name(format!("{filename}.sha256"));

    // Signatur über das Image: Signaturdatei schon jetzt holen – fehlt sie, gar nicht erst laden.
    let image_sig = match &img.signature {
        Some(sig) if sig.signs == SignedTarget::Image => {
            let url = sig.url.as_ref().expect("Validator: detached braucht url");
            Some((src.get_bytes(url).await?, sig.clone()))
        }
        _ => None,
    };
    check_cancel()?;

    // --- 2. Cache ------------------------------------------------------------------------------
    let mut from_cache = false;
    let mut actual = None;
    let mut size = 0;
    if dest.exists() {
        on_event(AcquireEvent::Stage {
            stage: Stage::CacheCheck,
        });
        let mut h = Hasher::new(algorithm);
        let total = tokio::fs::metadata(&dest).await.ok().map(|m| m.len());
        let n = hash_file(&dest, &mut h, cancel, |done| {
            on_event(AcquireEvent::Progress {
                stage: Stage::CacheCheck,
                done,
                total,
                bytes_per_sec: 0.0,
            })
        })
        .await?;
        let hash = h.finish_hex();
        let reference = match &expected {
            Some(e) => Some(e.clone()),
            None => tokio::fs::read_to_string(&sidecar)
                .await
                .ok()
                .map(|s| s.trim().to_owned()),
        };
        if reference.is_some_and(|r| hashes_equal(&hash, &r)) {
            from_cache = true;
            actual = Some(hash);
            size = n;
        } else {
            // Veraltet oder beschädigt → weg damit und neu laden.
            let _ = tokio::fs::remove_file(&dest).await;
        }
    }

    // --- 3. Download -----------------------------------------------------------------------------
    if !from_cache {
        on_event(AcquireEvent::Stage {
            stage: Stage::Download,
        });
        let d = download_pinned(
            src,
            &img.url,
            &dest,
            algorithm,
            origin_hosts.as_deref(),
            cancel,
            |p: Progress| {
                on_event(AcquireEvent::Progress {
                    stage: Stage::Download,
                    done: p.done,
                    total: p.total.or(img.size),
                    bytes_per_sec: p.bytes_per_sec,
                })
            },
        )
        .await?;
        actual = Some(d.hash);
        size = d.size;
    }

    // --- 4. Vergleich ----------------------------------------------------------------------------
    let actual = actual.expect("gesetzt in 2. oder 3.");
    match &expected {
        Some(expected) => {
            on_event(AcquireEvent::Stage {
                stage: Stage::Verify,
            });
            if !hashes_equal(&actual, expected) {
                let _ = tokio::fs::remove_file(&dest).await;
                return Err(AcquireError::ChecksumMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        // Nichts zu vergleichen – Hash nur festhalten (Fehler hier sind nicht schlimm).
        None => {
            let _ = tokio::fs::write(&sidecar, format!("{actual}\n")).await;
        }
    }

    // --- 5. Signatur über das ganze Image -------------------------------------------------------
    if let Some((signature, sig)) = image_sig {
        on_event(AcquireEvent::Stage {
            stage: Stage::ImageSignature,
        });
        // Sequoia liest synchron → in einen Thread auslagern, der das async-System nicht blockiert.
        let path = dest.clone();
        let certs = Arc::new(certs);
        let who = tokio::task::spawn_blocking(move || {
            let file =
                std::fs::File::open(&path).map_err(|e| GpgError::BadSignature(e.to_string()))?;
            gpg::verify_detached(
                std::io::BufReader::new(file),
                &signature,
                &certs,
                &sig.fingerprints,
                sig.allow_v3,
            )
        })
        .await
        .expect("Prüf-Thread abgestürzt");
        match who {
            Ok(who) => {
                on_event(AcquireEvent::Signed {
                    signer: who.clone(),
                });
                signer = Some(who);
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&dest).await;
                return Err(e.into());
            }
        }
    }

    let assurance = match (&signer, algorithm) {
        _ if origin_hosts.is_some() => Assurance::Origin,
        (Some(_), _) => Assurance::Signed,
        (None, HashAlgorithm::Md5) => Assurance::Weak,
        (None, _) => Assurance::Checksum,
    };
    Ok(Acquired {
        path: dest,
        filename,
        size,
        algorithm,
        hash: actual,
        signer,
        assurance,
        from_cache,
    })
}
