//! Große Dateien herunterladen: mit Fortschritt, Abbrechen und Fortsetzen.
//!
//! ```text
//!  ubuntu.iso.part      ← wird beschrieben (bleibt bei Abbruch/Fehler liegen → Fortsetzen)
//!  ubuntu.iso.part.json ← merkt sich URL + ETag/Last-Modified der Teil-Datei
//!  ubuntu.iso           ← erst nach vollständigem Download umbenannt
//! ```
//!
//! **Fortsetzen** nutzt HTTP-Range: „Gib mir ab Byte N“. Damit wir nicht versehentlich den Anfang
//! von Version A mit dem Ende von Version B zusammenkleben, schicken wir `If-Range` mit dem
//! gespeicherten ETag mit. Hat sich die Datei auf dem Server geändert, schickt er sie komplett neu.
//!
//! Der Hash wird **während** des Downloads berechnet (beim Fortsetzen wird der vorhandene Teil
//! zuerst eingelesen). So muss die fertige Datei nicht noch einmal komplett gelesen werden.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::Url;

use crate::catalog::HashAlgorithm;
use crate::http::{FetchError, HttpClient, require_https};
use crate::verify::sums::Hasher;

/// Wie oft höchstens Fortschritt gemeldet wird (die GUI braucht keine 1000 Updates pro Sekunde).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
const READ_BUF: usize = 1024 * 1024;

/// Fortschritt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
    /// Geglättete Geschwindigkeit in Bytes/s.
    pub bytes_per_sec: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("Dateifehler: {0}")]
    Io(String),
    #[error("abgebrochen")]
    Cancelled,
    #[error("Download unvollständig: {got} von {expected} Bytes")]
    Incomplete { got: u64, expected: u64 },
    #[error("Nicht genug Speicherplatz: {need} Bytes nötig, {available} frei")]
    NoSpace { need: u64, available: u64 },
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        DownloadError::Io(e.to_string())
    }
}

/// Antwort auf eine (Range-)Anfrage.
pub struct Opened<B> {
    /// `true`, wenn der Server ab der gewünschten Position liefert (206).
    pub resumed: bool,
    /// Gesamtgröße der Datei, falls bekannt.
    pub total: Option<u64>,
    /// ETag oder Last-Modified – für späteres `If-Range`.
    pub validator: Option<String>,
    pub body: B,
}

/// Ein Strom von Datenstücken.
pub trait Body: Send {
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Vec<u8>>, FetchError>> + Send;
}

/// Quelle für Downloads mit Range-Unterstützung (echt: HTTP; in Tests: Fake).
pub trait RangeSource: Send + Sync {
    type Body: Body;
    fn open(
        &self,
        url: &Url,
        from: u64,
        if_range: Option<&str>,
    ) -> impl Future<Output = Result<Opened<Self::Body>, FetchError>> + Send;
}

/// Ergebnis eines erfolgreichen Downloads.
#[derive(Debug, Clone, PartialEq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub size: u64,
    /// Hash über die komplette Datei (Hex, klein).
    pub hash: String,
}

#[derive(Serialize, Deserialize)]
struct PartMeta {
    url: String,
    validator: Option<String>,
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Lädt `url` nach `dest` herunter und berechnet dabei den Hash.
pub async fn download<S: RangeSource>(
    src: &S,
    url: &Url,
    dest: &Path,
    algorithm: HashAlgorithm,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress) + Send,
) -> Result<Downloaded, DownloadError> {
    let part = with_suffix(dest, ".part");
    let meta_path = with_suffix(dest, ".part.json");
    if let Some(dir) = dest.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }

    // 1) Vorhandene Teil-Datei zu DIESER URL? Dann einlesen und weitermachen.
    let meta: Option<PartMeta> = tokio::fs::read(&meta_path)
        .await
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let mut hasher = Hasher::new(algorithm);
    let mut existing = 0u64;
    let resumable = meta.as_ref().is_some_and(|m| m.url == url.as_str()) && part.exists();
    if resumable {
        // Fortschritt melden, damit die GUI beim Einlesen großer Teil-Dateien nicht „hängt“.
        // `total: None`, weil die Gesamtgröße erst nach der Anfrage bekannt ist.
        existing = hash_file(&part, &mut hasher, cancel, |done| {
            on_progress(Progress {
                done,
                total: None,
                bytes_per_sec: 0.0,
            })
        })
        .await?;
    } else {
        let _ = tokio::fs::remove_file(&part).await;
    }
    let validator = meta.and_then(|m| m.validator).filter(|_| existing > 0);

    // 2) Anfrage.
    let mut opened = src.open(url, existing, validator.as_deref()).await?;
    if existing > 0 && !opened.resumed {
        // Server liefert komplett neu (kein Range-Support oder Datei geändert) → von vorn.
        existing = 0;
        hasher = Hasher::new(algorithm);
    }
    // Passt die Datei überhaupt noch auf die Platte? Lieber jetzt abbrechen als bei 95 %.
    if let (Some(total), Some(dir)) = (opened.total, dest.parent())
        && let Ok(available) = crate::disk::available_space(dir)
    {
        let need = total.saturating_sub(existing);
        if need.saturating_add(crate::disk::RESERVE) > available {
            return Err(DownloadError::NoSpace { need, available });
        }
    }
    let meta = PartMeta {
        url: url.to_string(),
        validator: opened.validator.clone(),
    };
    tokio::fs::write(
        &meta_path,
        serde_json::to_vec(&meta).expect("serialisierbar"),
    )
    .await?;

    let mut file = if existing > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .await?
    } else {
        tokio::fs::File::create(&part).await?
    };

    // 3) Daten empfangen.
    let mut done = existing;
    let mut speed = SpeedMeter::new(done);
    on_progress(Progress {
        done,
        total: opened.total,
        bytes_per_sec: 0.0,
    });
    loop {
        if cancel.load(Ordering::Relaxed) {
            file.flush().await?;
            return Err(DownloadError::Cancelled);
        }
        let chunk = match opened.body.next_chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                // Teil-Datei behalten → beim nächsten Versuch geht es hier weiter.
                file.flush().await?;
                return Err(e.into());
            }
        };
        file.write_all(&chunk).await?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if let Some(bps) = speed.tick(done) {
            on_progress(Progress {
                done,
                total: opened.total,
                bytes_per_sec: bps,
            });
        }
    }
    file.flush().await?;
    file.sync_all().await?; // wirklich auf die Platte, bevor wir „fertig“ sagen
    drop(file);

    if let Some(total) = opened.total
        && done != total
    {
        return Err(DownloadError::Incomplete {
            got: done,
            expected: total,
        });
    }
    on_progress(Progress {
        done,
        total: Some(done),
        bytes_per_sec: speed.current(),
    });

    tokio::fs::rename(&part, dest).await?;
    let _ = tokio::fs::remove_file(&meta_path).await;
    Ok(Downloaded {
        path: dest.to_owned(),
        size: done,
        hash: hasher.finish_hex(),
    })
}

/// Liest eine Datei komplett durch den Hasher. Gibt die Größe zurück.
/// `on_bytes` bekommt regelmäßig die bisher gelesene Menge (für Fortschrittsanzeigen).
pub async fn hash_file(
    path: &Path,
    hasher: &mut Hasher,
    cancel: &AtomicBool,
    mut on_bytes: impl FnMut(u64) + Send,
) -> Result<u64, DownloadError> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; READ_BUF];
    let mut total = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = f.read(&mut buf).await?;
        if n == 0 {
            return Ok(total);
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        if total % (64 * READ_BUF as u64) < n as u64 {
            on_bytes(total);
        }
    }
}

/// Misst die Geschwindigkeit, geglättet (sonst springt die Anzeige wild hin und her).
struct SpeedMeter {
    last_at: Instant,
    last_bytes: u64,
    bps: f64,
}

impl SpeedMeter {
    fn new(start: u64) -> Self {
        Self {
            last_at: Instant::now(),
            last_bytes: start,
            bps: 0.0,
        }
    }
    /// Liefert alle `PROGRESS_INTERVAL` eine neue Geschwindigkeit, sonst `None`.
    fn tick(&mut self, now_bytes: u64) -> Option<f64> {
        let dt = self.last_at.elapsed();
        if dt < PROGRESS_INTERVAL {
            return None;
        }
        let sample = (now_bytes - self.last_bytes) as f64 / dt.as_secs_f64();
        // Exponentiell gleitender Mittelwert: 30 % neuer Wert, 70 % Verlauf.
        self.bps = if self.bps == 0.0 {
            sample
        } else {
            0.3 * sample + 0.7 * self.bps
        };
        self.last_at = Instant::now();
        self.last_bytes = now_bytes;
        Some(self.bps)
    }
    fn current(&self) -> f64 {
        self.bps
    }
}

// --- echte HTTP-Quelle ------------------------------------------------------------------------

pub struct HttpBody(reqwest::Response, String);

impl Body for HttpBody {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FetchError> {
        self.0
            .chunk()
            .await
            .map(|c| c.map(|b| b.to_vec()))
            .map_err(|e| FetchError::Network {
                url: self.1.clone(),
                message: e.without_url().to_string(),
            })
    }
}

impl RangeSource for HttpClient {
    type Body = HttpBody;

    async fn open(
        &self,
        url: &Url,
        from: u64,
        if_range: Option<&str>,
    ) -> Result<Opened<HttpBody>, FetchError> {
        use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
        require_https(url)?;
        let net = |e: reqwest::Error| FetchError::Network {
            url: url.to_string(),
            message: e.without_url().to_string(),
        };

        let mut req = self.inner().get(url.clone());
        if from > 0 {
            req = req.header(RANGE, format!("bytes={from}-"));
            if let Some(v) = if_range {
                req = req.header(IF_RANGE, v);
            }
        }
        let resp = req.send().await.map_err(net)?;
        let status = resp.status();

        let header = |name| {
            resp.headers()
                .get(name)
                .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
        };
        // Nur „starke“ ETags taugen für If-Range (schwache beginnen mit W/).
        let validator = header(ETAG)
            .filter(|e| !e.starts_with("W/"))
            .or_else(|| header(LAST_MODIFIED))
            .map(str::to_owned);

        let (resumed, total) = match status.as_u16() {
            206 => {
                // Content-Range: bytes 1000-1999/5000  → Gesamtgröße 5000
                let total = header(CONTENT_RANGE)
                    .and_then(|v| v.rsplit('/').next())
                    .and_then(|t| t.parse().ok());
                (true, total)
            }
            200 => (false, resp.content_length()),
            s => {
                return Err(FetchError::Status {
                    url: url.to_string(),
                    status: s,
                });
            }
        };
        Ok(Opened {
            resumed,
            total,
            validator,
            body: HttpBody(resp, url.to_string()),
        })
    }
}

// --- Fake für Tests ---------------------------------------------------------------------------

#[cfg(any(test, feature = "test-util"))]
pub mod fake {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use super::*;

    /// Liefert `data` aus. Optional: Abbruch nach N Bytes, kein Range-Support, anderer ETag.
    pub struct FakeSource {
        pub data: Vec<u8>,
        pub chunk: usize,
        pub fail_after: Mutex<Option<u64>>,
        pub supports_range: bool,
        pub etag: String,
        pub opens: AtomicUsize,
        pub last_from: Mutex<u64>,
        /// Gemeldete Gesamtgröße überschreiben (z. B. riesig, um „Platte voll“ zu testen).
        pub claimed_total: Option<u64>,
    }

    impl FakeSource {
        pub fn new(data: Vec<u8>) -> Self {
            Self {
                data,
                chunk: 1000,
                fail_after: Mutex::new(None),
                supports_range: true,
                etag: "\"v1\"".into(),
                opens: AtomicUsize::new(0),
                last_from: Mutex::new(0),
                claimed_total: None,
            }
        }
    }

    pub struct FakeBody {
        chunks: std::vec::IntoIter<Vec<u8>>,
        fail: bool,
    }

    impl Body for FakeBody {
        async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, FetchError> {
            match self.chunks.next() {
                Some(c) => Ok(Some(c)),
                None if self.fail => Err(FetchError::Network {
                    url: "fake".into(),
                    message: "Verbindung weg".into(),
                }),
                None => Ok(None),
            }
        }
    }

    impl RangeSource for FakeSource {
        type Body = FakeBody;
        async fn open(
            &self,
            _url: &Url,
            from: u64,
            if_range: Option<&str>,
        ) -> Result<Opened<FakeBody>, FetchError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            *self.last_from.lock().unwrap() = from;
            let honor = from > 0 && self.supports_range && if_range == Some(self.etag.as_str());
            let start = if honor { from as usize } else { 0 };
            let fail_after = self.fail_after.lock().unwrap().take();
            let end = fail_after.map_or(self.data.len(), |n| (n as usize).min(self.data.len()));
            let chunks = self.data[start..end.max(start)]
                .chunks(self.chunk)
                .map(<[u8]>::to_vec)
                .collect::<Vec<_>>();
            Ok(Opened {
                resumed: honor,
                total: Some(self.claimed_total.unwrap_or(self.data.len() as u64)),
                validator: Some(self.etag.clone()),
                body: FakeBody {
                    chunks: chunks.into_iter(),
                    fail: fail_after.is_some(),
                },
            })
        }
    }
}
