//! Die neueste Version einer Variante ermitteln („resolven“).
//!
//! Ergebnis ist ein [`ResolvedImage`]: konkrete Download-URL, Version, Dateiname und
//! woher die Prüfsumme/Signatur kommt. Geladen wird hier noch nichts außer kleinen Textseiten.

mod listing;
pub mod version;

use std::future::Future;
use std::pin::Pin;

use regex::Regex;
use serde::Serialize;
use url::Url;

pub use listing::extract_links;

use crate::catalog::{
    Checksum, Compression, FlashMode, HashAlgorithm, JsonFields, Select, SignatureKind,
    SignedTarget, Source, Step, Variant,
};
use crate::http::{FetchError, Fetcher, require_https};

/// Wie viele Kandidaten pro Schritt höchstens ausprobiert werden, wenn der höchste eine Sackgasse ist.
/// Begrenzt, damit ein kaputter Katalogeintrag keinen Server mit Anfragen flutet.
const MAX_BACKTRACK: usize = 3;

/// Das Ergebnis: alles, was Phase 3 zum Herunterladen und Prüfen braucht.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedImage {
    pub variant_id: String,
    pub version: String,
    /// Build-Nummer, falls die Quelle eine liefert (Pop!_OS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    pub filename: String,
    pub url: Url,
    /// Größe in Bytes, falls bekannt.
    pub size: Option<u64>,
    pub checksum: ResolvedChecksum,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<ResolvedSignature>,
    pub compression: Compression,
    pub flash_mode: FlashMode,
}

/// Woher die erwartete Prüfsumme kommt.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResolvedChecksum {
    /// Aus dieser Datei lesen (Zeile mit `filename` suchen).
    File { url: Url, algorithm: HashAlgorithm },
    /// Direkt bekannt (aus JSON-API).
    Value {
        value: String,
        algorithm: HashAlgorithm,
    },
}

impl ResolvedChecksum {
    pub fn algorithm(&self) -> HashAlgorithm {
        match self {
            ResolvedChecksum::File { algorithm, .. }
            | ResolvedChecksum::Value { algorithm, .. } => *algorithm,
        }
    }
}

/// Konkrete Signaturdaten.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedSignature {
    pub kind: SignatureKind,
    pub signs: SignedTarget,
    /// URL der Signaturdatei; `None` bei Clearsign (dann ist die Prüfsummendatei signiert).
    pub url: Option<Url>,
    pub fingerprints: Vec<String>,
    pub keyring_url: Option<Url>,
    pub allow_v3: bool,
}

/// Was beim Ermitteln schiefgehen kann.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error("unter {url} passt nichts zu `{pattern}`")]
    NoMatch { url: String, pattern: String },
    #[error("ungültige Regex `{0}`")]
    BadPattern(String),
    #[error("ungültige URL `{0}`")]
    BadUrl(String),
    #[error("Antwort von {url} ist kein gültiges JSON: {message}")]
    Json { url: String, message: String },
    #[error("Feld `{pointer}` fehlt in der Antwort von {url}")]
    MissingField { url: String, pointer: String },
    #[error("Prüfsumme `{value}` passt nicht zu {algorithm:?}")]
    BadChecksumValue {
        value: String,
        algorithm: HashAlgorithm,
    },
    #[error("die Quelle `{0}` wird erst in einer späteren Version unterstützt")]
    NotSupportedYet(&'static str),
    #[error("dieses Image lädt man selbst herunter und wählt es dann aus")]
    Manual,
}

impl ResolveError {
    /// Sackgasse? Dann darf der nächste Kandidat probiert werden.
    fn is_dead_end(&self) -> bool {
        match self {
            ResolveError::NoMatch { .. } => true,
            ResolveError::Fetch(e) => e.is_not_found(),
            _ => false,
        }
    }
}

/// Ermittelt die neueste Version einer Variante.
pub async fn resolve<F: Fetcher>(f: &F, variant: &Variant) -> Result<ResolvedImage, ResolveError> {
    let found = match &variant.source {
        Source::DirectoryListing { base_url, steps } => {
            let mut base = parse_https(base_url)?;
            ensure_dir(&mut base);
            walk(f, base, steps, None).await?
        }
        Source::JsonApi { url, fields } => json_api(f, &parse_https(url)?, fields).await?,
        Source::GithubRelease {
            repo,
            asset_pattern,
            include_prereleases,
        } => github_release(f, repo, asset_pattern, *include_prereleases).await?,
        // Wird vom Nutzer selbst heruntergeladen – es gibt nichts zu ermitteln.
        Source::Manual { .. } => return Err(ResolveError::Manual),
    };

    let image_dir = found
        .url
        .join("./")
        .map_err(|_| ResolveError::BadUrl(found.url.to_string()))?;
    let vars = Vars {
        filename: &found.filename,
        version: &found.version,
    };

    let checksum = match &variant.checksum {
        Checksum::File {
            url: Some(t),
            algorithm,
            ..
        } => ResolvedChecksum::File {
            url: join_template(&image_dir, t, &vars)?,
            algorithm: *algorithm,
        },
        Checksum::File {
            pattern: Some(p),
            algorithm,
            ..
        } => {
            let re = Regex::new(p).map_err(|_| ResolveError::BadPattern(p.clone()))?;
            let link = found
                .listing
                .iter()
                .find(|l| re.is_match(l))
                .ok_or_else(|| ResolveError::NoMatch {
                    url: found.listing_url.to_string(),
                    pattern: p.clone(),
                })?;
            ResolvedChecksum::File {
                url: join_relative(&found.listing_url, link)?,
                algorithm: *algorithm,
            }
        }
        Checksum::File { .. } | Checksum::PageListing { .. } => {
            unreachable!("vom Validator ausgeschlossen")
        }
        Checksum::FromSource { algorithm } => {
            let value = found.checksum.clone().ok_or(ResolveError::MissingField {
                url: found.url.to_string(),
                pointer: "checksum".into(),
            })?;
            check_hex(&value, *algorithm)?;
            ResolvedChecksum::Value {
                value: value.to_ascii_lowercase(),
                algorithm: *algorithm,
            }
        }
    };

    let signature = match &variant.signature {
        None => None,
        Some(s) => Some(ResolvedSignature {
            kind: s.kind,
            signs: s.signs,
            url: s
                .url
                .as_ref()
                .map(|t| join_template(&image_dir, t, &vars))
                .transpose()?,
            fingerprints: s.fingerprints.clone(),
            keyring_url: s.keyring_url.as_deref().map(parse_https).transpose()?,
            allow_v3: s.allow_v3,
        }),
    };

    // Größe: aus der Quelle oder per HEAD. Fehler hier sind egal – die Größe ist nur Zusatzinfo.
    let size = match found.size {
        Some(s) => Some(s),
        None => f.content_length(&found.url).await.ok().flatten(),
    };

    Ok(ResolvedImage {
        variant_id: variant.id.clone(),
        version: found.version,
        build: found.build,
        filename: found.filename,
        url: found.url,
        size,
        checksum,
        signature,
        compression: variant.compression,
        flash_mode: variant.flash_mode,
    })
}

/// Zwischenergebnis der Quellen.
#[derive(Debug)]
struct Found {
    url: Url,
    filename: String,
    version: String,
    build: Option<String>,
    size: Option<u64>,
    checksum: Option<String>,
    /// Das Listing, in dem das Image gefunden wurde (für `checksum.match`).
    listing_url: Url,
    listing: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// directory_listing

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Läuft die Schritte rekursiv ab. Rekursion in `async fn` braucht `Box::pin`, weil der
/// Compiler sonst die Größe des Futures nicht kennt (es würde sich selbst enthalten).
fn walk<'a, F: Fetcher>(
    f: &'a F,
    dir: Url,
    steps: &'a [Step],
    version: Option<String>,
) -> BoxFut<'a, Result<Found, ResolveError>> {
    Box::pin(async move {
        let (step, rest) = steps
            .split_first()
            .expect("Validator: steps nicht leer, letzter ist match");
        match step {
            Step::Path { path } => {
                let mut next = join_relative(&dir, path)?;
                ensure_dir(&mut next);
                walk(f, next, rest, version).await
            }
            Step::Match { pattern, select } => {
                let re =
                    Regex::new(pattern).map_err(|_| ResolveError::BadPattern(pattern.clone()))?;
                let html = f.get_text(&dir).await?;
                let links = extract_links(&html, &dir);
                let candidates = pick(&re, &links, *select);
                if candidates.is_empty() {
                    return Err(ResolveError::NoMatch {
                        url: dir.to_string(),
                        pattern: pattern.clone(),
                    });
                }

                if rest.is_empty() {
                    // Letzter Schritt: der beste Treffer ist das Image.
                    let (link, v) = candidates.into_iter().next().expect("nicht leer");
                    let url = join_relative(&dir, &link)?;
                    let filename = link.rsplit('/').next().unwrap_or(&link).to_owned();
                    let version = v.or(version).unwrap_or_else(|| filename.clone());
                    return Ok(Found {
                        url,
                        filename,
                        version,
                        build: None,
                        size: None,
                        checksum: None,
                        listing_url: dir,
                        listing: links,
                    });
                }

                // Zwischenschritt: absteigen, bei Sackgasse nächsten Kandidaten probieren.
                let mut last_err = None;
                for (link, v) in candidates.into_iter().take(MAX_BACKTRACK) {
                    let mut next = join_relative(&dir, &link)?;
                    ensure_dir(&mut next);
                    match walk(f, next, rest, v.or_else(|| version.clone())).await {
                        Ok(found) => return Ok(found),
                        Err(e) if e.is_dead_end() => last_err = Some(e),
                        Err(e) => return Err(e),
                    }
                }
                Err(last_err.expect("mindestens ein Kandidat probiert"))
            }
        }
    })
}

/// Treffer filtern und sortieren (bester zuerst). Liefert (Link, Version aus `(?P<version>…)`).
fn pick(re: &Regex, links: &[String], select: Select) -> Vec<(String, Option<String>)> {
    let mut hits: Vec<(String, Option<String>)> = links
        .iter()
        .filter_map(|l| {
            let caps = re.captures(l)?;
            Some((
                l.clone(),
                caps.name("version").map(|m| m.as_str().to_owned()),
            ))
        })
        .collect();
    hits.sort_by(|(la, va), (lb, vb)| {
        let ord = match select {
            Select::HighestVersion => {
                version::compare(va.as_deref().unwrap_or(la), vb.as_deref().unwrap_or(lb))
            }
            Select::Lexical => la.cmp(lb),
        };
        // Absteigend; bei gleicher Version entscheidet der Link (stabil, deterministisch).
        ord.then_with(|| la.cmp(lb)).reverse()
    });
    hits
}

// ---------------------------------------------------------------------------------------------
// json_api

async fn json_api<F: Fetcher>(
    f: &F,
    url: &Url,
    fields: &JsonFields,
) -> Result<Found, ResolveError> {
    let text = f.get_text(url).await?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| ResolveError::Json {
        url: url.to_string(),
        message: e.to_string(),
    })?;

    let get = |pointer: &str| -> Option<String> {
        match json.pointer(pointer)? {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let required = |pointer: &str| {
        get(pointer).ok_or_else(|| ResolveError::MissingField {
            url: url.to_string(),
            pointer: pointer.into(),
        })
    };

    let image_url = parse_https(&required(&fields.url)?)?;
    let filename = image_url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .unwrap_or_default()
        .to_owned();
    Ok(Found {
        filename,
        version: required(&fields.version)?,
        build: fields.build.as_deref().and_then(get),
        size: fields
            .size
            .as_deref()
            .and_then(get)
            .and_then(|s| s.parse().ok()),
        checksum: fields.checksum.as_deref().and_then(get),
        listing_url: image_url.clone(),
        listing: Vec::new(),
        url: image_url,
    })
}

// ---------------------------------------------------------------------------------------------
// github_release

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    assets: Vec<GhAsset>,
}

#[derive(serde::Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

async fn github_release<F: Fetcher>(
    f: &F,
    repo: &str,
    asset_pattern: &str,
    include_prereleases: bool,
) -> Result<Found, ResolveError> {
    let re =
        Regex::new(asset_pattern).map_err(|_| ResolveError::BadPattern(asset_pattern.into()))?;
    let api = parse_https(&format!(
        "https://api.github.com/repos/{repo}/releases?per_page=20"
    ))?;
    let text = f.get_text(&api).await?;
    let releases: Vec<GhRelease> = serde_json::from_str(&text).map_err(|e| ResolveError::Json {
        url: api.to_string(),
        message: e.to_string(),
    })?;

    // Die API liefert neueste zuerst. Erstes passendes Release mit passendem Asset gewinnt.
    for rel in releases
        .iter()
        .filter(|r| !r.draft && (include_prereleases || !r.prerelease))
    {
        if let Some(asset) = rel.assets.iter().find(|a| re.is_match(&a.name)) {
            let url = parse_https(&asset.browser_download_url)?;
            return Ok(Found {
                filename: asset.name.clone(),
                version: rel.tag_name.trim_start_matches('v').to_owned(),
                build: None,
                size: Some(asset.size),
                checksum: None,
                listing_url: url
                    .join("./")
                    .map_err(|_| ResolveError::BadUrl(url.to_string()))?,
                listing: rel.assets.iter().map(|a| a.name.clone()).collect(),
                url,
            });
        }
    }
    Err(ResolveError::NoMatch {
        url: api.to_string(),
        pattern: asset_pattern.into(),
    })
}

// ---------------------------------------------------------------------------------------------
// Hilfsfunktionen

fn parse_https(s: &str) -> Result<Url, ResolveError> {
    let url = Url::parse(s).map_err(|_| ResolveError::BadUrl(s.into()))?;
    require_https(&url)?;
    Ok(url)
}

/// Sorgt für `/` am Ende, sonst würde `join` den letzten Pfadteil ersetzen statt anzuhängen.
fn ensure_dir(url: &mut Url) {
    if !url.path().ends_with('/') {
        let p = format!("{}/", url.path());
        url.set_path(&p);
    }
}

/// Relativen Pfad (z. B. `44/` oder `TrueNAS-SCALE-Goldeye/25.10.7/x.iso`) an ein Verzeichnis hängen.
/// Segmentweise, damit Sonderzeichen wie `#`, `?` oder `%` in Dateinamen korrekt kodiert werden.
fn join_relative(dir: &Url, rel: &str) -> Result<Url, ResolveError> {
    if rel.split('/').any(|seg| seg == "..") {
        return Err(ResolveError::BadUrl(rel.into()));
    }
    let mut url = dir.clone();
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| ResolveError::BadUrl(dir.to_string()))?;
        segs.pop_if_empty();
        segs.extend(rel.split('/'));
    }
    Ok(url)
}

struct Vars<'a> {
    filename: &'a str,
    version: &'a str,
}

/// Platzhalter ersetzen und relativ zum Image-Verzeichnis auflösen (oder absolute HTTPS-URL).
fn join_template(image_dir: &Url, template: &str, vars: &Vars) -> Result<Url, ResolveError> {
    let s = template
        .replace("{filename}", vars.filename)
        .replace("{version}", vars.version);
    if s.contains("://") {
        parse_https(&s)
    } else {
        join_relative(image_dir, &s)
    }
}

fn check_hex(value: &str, algorithm: HashAlgorithm) -> Result<(), ResolveError> {
    let len = match algorithm {
        HashAlgorithm::Sha256 => 64,
        HashAlgorithm::Sha512 => 128,
        HashAlgorithm::Md5 => 32,
    };
    if value.len() == len && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ResolveError::BadChecksumValue {
            value: value.into(),
            algorithm,
        })
    }
}
