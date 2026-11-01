//! HTTP-Zugriff.
//!
//! Alles, was BootForge aus dem Netz holt, läuft über das Trait [`Fetcher`]. Die echte
//! Implementierung ist [`HttpClient`]; in Tests ersetzen wir sie durch einen Fake, der
//! vorbereitete Antworten liefert. So laufen die Tests ohne Internet und immer gleich.

use std::future::Future;
use std::time::Duration;

use url::Url;

/// Größte erlaubte Textantwort (Listings, Prüfsummen, Katalog).
/// `download.truenas.com` liefert ein ~12 MB großes Listing, daher großzügig.
pub const MAX_TEXT_BYTES: u64 = 32 * 1024 * 1024;

/// Fehler beim Abrufen.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("nur HTTPS erlaubt: {0}")]
    NotHttps(String),
    #[error("{url} antwortet mit HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("Antwort von {url} ist zu groß (> {limit} Bytes)")]
    TooLarge { url: String, limit: u64 },
    #[error("Netzwerkfehler bei {url}: {message}")]
    Network { url: String, message: String },
}

impl FetchError {
    /// Gibt es die Ressource nicht (404/410)? Dann lohnt es sich, eine andere Version zu probieren.
    pub fn is_not_found(&self) -> bool {
        matches!(
            self,
            FetchError::Status {
                status: 404 | 410,
                ..
            }
        )
    }
}

/// Ergebnis einer bedingten Anfrage (mit ETag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conditional {
    /// Der Server meldet „304 Not Modified“: unsere Kopie ist aktuell.
    NotModified,
    /// Neuer Inhalt.
    Body { text: String, etag: Option<String> },
}

/// Abstraktion über HTTP – siehe Modul-Doku.
pub trait Fetcher: Send + Sync {
    /// Text laden. Mit `etag` wird „If-None-Match“ gesendet.
    fn get(
        &self,
        url: &Url,
        etag: Option<&str>,
    ) -> impl Future<Output = Result<Conditional, FetchError>> + Send;

    /// Dateigröße per HEAD-Anfrage (falls der Server sie verrät).
    fn content_length(
        &self,
        url: &Url,
    ) -> impl Future<Output = Result<Option<u64>, FetchError>> + Send;

    /// Text ohne ETag laden.
    fn get_text(&self, url: &Url) -> impl Future<Output = Result<String, FetchError>> + Send {
        async move {
            match self.get(url, None).await? {
                Conditional::Body { text, .. } => Ok(text),
                // Ohne If-None-Match darf kein 304 kommen – falls doch, ist der Server kaputt.
                Conditional::NotModified => Err(FetchError::Status {
                    url: url.to_string(),
                    status: 304,
                }),
            }
        }
    }
}

/// Prüft, dass eine URL HTTPS ist. Zentrale Stelle, damit die Regel nirgends vergessen wird.
pub fn require_https(url: &Url) -> Result<(), FetchError> {
    if url.scheme() == "https" {
        Ok(())
    } else {
        Err(FetchError::NotHttps(url.to_string()))
    }
}

/// Der echte HTTP-Client.
#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> Result<Self, FetchError> {
        let client = reqwest::Client::builder()
            // Wichtig: verweigert auch Weiterleitungen von HTTPS auf HTTP.
            .https_only(true)
            .user_agent(concat!(
                "BootForge/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/xHellPhoenix/bootforge)"
            ))
            .connect_timeout(Duration::from_secs(10))
            // Zeit ohne neue Daten, nicht Gesamtzeit (große Listings dürfen länger dauern).
            .read_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(|e| FetchError::Network {
                url: String::new(),
                message: e.to_string(),
            })?;
        Ok(Self { client })
    }

    /// Zugriff auf den inneren Client (für Downloads in Phase 3).
    pub fn inner(&self) -> &reqwest::Client {
        &self.client
    }
}

fn network(url: &Url, e: reqwest::Error) -> FetchError {
    // `without_url`, weil wir die URL selbst ausgeben – sonst steht sie doppelt im Text.
    // Die eigentliche Ursache (DNS, TLS, Timeout …) steckt oft erst in der `source()`-Kette.
    let e = e.without_url();
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        message.push_str(": ");
        message.push_str(&s.to_string());
        source = s.source();
    }
    FetchError::Network {
        url: url.to_string(),
        message,
    }
}

impl HttpClient {
    async fn get_once(&self, url: &Url, etag: Option<&str>) -> Result<Conditional, FetchError> {
        let mut req = self.client.get(url.clone());
        if let Some(etag) = etag {
            req = req.header(reqwest::header::IF_NONE_MATCH, etag);
        }
        let mut resp = req.send().await.map_err(|e| network(url, e))?;

        let status = resp.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(Conditional::NotModified);
        }
        if !status.is_success() {
            return Err(FetchError::Status {
                url: url.to_string(),
                status: status.as_u16(),
            });
        }
        if resp
            .content_length()
            .is_some_and(|len| len > MAX_TEXT_BYTES)
        {
            return Err(FetchError::TooLarge {
                url: url.to_string(),
                limit: MAX_TEXT_BYTES,
            });
        }
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        // Stückweise lesen und mitzählen: Content-Length kann fehlen oder lügen.
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| network(url, e))? {
            body.extend_from_slice(&chunk);
            if body.len() as u64 > MAX_TEXT_BYTES {
                return Err(FetchError::TooLarge {
                    url: url.to_string(),
                    limit: MAX_TEXT_BYTES,
                });
            }
        }
        Ok(Conditional::Body {
            text: String::from_utf8_lossy(&body).into_owned(),
            etag,
        })
    }
}

impl Fetcher for HttpClient {
    async fn get(&self, url: &Url, etag: Option<&str>) -> Result<Conditional, FetchError> {
        require_https(url)?;
        // Ein zweiter Versuch bei Netzwerkfehlern (Verbindungsabbruch, Timeout). Nicht bei
        // HTTP-Fehlern wie 404 – die ändern sich durch Wiederholen nicht.
        match self.get_once(url, etag).await {
            Err(FetchError::Network { .. }) => {
                tokio::time::sleep(Duration::from_millis(800)).await;
                self.get_once(url, etag).await
            }
            other => other,
        }
    }

    async fn content_length(&self, url: &Url) -> Result<Option<u64>, FetchError> {
        require_https(url)?;
        let resp = self
            .client
            .head(url.clone())
            .send()
            .await
            .map_err(|e| network(url, e))?;
        if !resp.status().is_success() {
            return Err(FetchError::Status {
                url: url.to_string(),
                status: resp.status().as_u16(),
            });
        }
        // Bei HEAD ist `content_length()` von reqwest immer 0 → Header direkt lesen.
        Ok(resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok()))
    }
}

/// Fake-Fetcher für Tests: URL → Antwort.
#[cfg(any(test, feature = "test-util"))]
pub mod fake {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct FakeFetcher {
        pages: HashMap<String, (String, Option<String>)>,
        sizes: HashMap<String, u64>,
        offline: bool,
        /// Alle abgefragten URLs (zum Prüfen in Tests).
        pub requests: Mutex<Vec<String>>,
    }

    impl FakeFetcher {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn page(mut self, url: &str, body: &str) -> Self {
            self.pages.insert(url.into(), (body.into(), None));
            self
        }
        pub fn page_with_etag(mut self, url: &str, body: &str, etag: &str) -> Self {
            self.pages
                .insert(url.into(), (body.into(), Some(etag.into())));
            self
        }
        pub fn size(mut self, url: &str, size: u64) -> Self {
            self.sizes.insert(url.into(), size);
            self
        }
        pub fn offline() -> Self {
            Self {
                offline: true,
                ..Self::default()
            }
        }
    }

    impl Fetcher for FakeFetcher {
        async fn get(&self, url: &Url, etag: Option<&str>) -> Result<Conditional, FetchError> {
            require_https(url)?;
            self.requests.lock().unwrap().push(url.to_string());
            if self.offline {
                return Err(FetchError::Network {
                    url: url.to_string(),
                    message: "offline".into(),
                });
            }
            match self.pages.get(url.as_str()) {
                Some((_, Some(e))) if Some(e.as_str()) == etag => Ok(Conditional::NotModified),
                Some((text, e)) => Ok(Conditional::Body {
                    text: text.clone(),
                    etag: e.clone(),
                }),
                None => Err(FetchError::Status {
                    url: url.to_string(),
                    status: 404,
                }),
            }
        }
        async fn content_length(&self, url: &Url) -> Result<Option<u64>, FetchError> {
            Ok(self.sizes.get(url.as_str()).copied())
        }
    }
}
