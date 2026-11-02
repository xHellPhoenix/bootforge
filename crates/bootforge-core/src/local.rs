//! Eigene Image-Dateien: prüfen und erkennen.
//!
//! Für Images, die man selbst herunterlädt (Windows von Microsofts Seite, oder irgendein
//! eigenes ISO). Die Regel „niemals ohne passende Prüfsumme“ gilt auch hier – die Prüfsumme kommt
//! entweder vom Nutzer (vom Hersteller kopiert) oder von einer HTTPS-Seite des Herstellers.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::acquire::{AcquireError, AcquireEvent, Acquired, Assurance, Stage, safe_filename};
use crate::catalog::HashAlgorithm;
use crate::download::hash_file;
use crate::http::Fetcher;
use crate::verify::sums::{Hasher, hashes_equal, hex_len};

/// Wogegen die Datei geprüft wird.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Expected {
    /// Vom Nutzer eingegeben (vom Hersteller kopiert).
    Hash {
        algorithm: HashAlgorithm,
        value: String,
    },
    /// Muss auf dieser HTTPS-Seite stehen (z. B. Microsofts Tabelle).
    PageListing { url: Url, algorithm: HashAlgorithm },
}

impl Expected {
    fn algorithm(&self) -> HashAlgorithm {
        match self {
            Expected::Hash { algorithm, .. } | Expected::PageListing { algorithm, .. } => {
                *algorithm
            }
        }
    }
}

/// Eingegebene Prüfsumme verstehen: Hex beliebiger Schreibweise, optional mit Präfix
/// („sha256:…“, „SHA256 …“), Leerzeichen egal. Der Algorithmus ergibt sich aus der Länge.
pub fn parse_user_hash(input: &str) -> Option<(HashAlgorithm, String)> {
    let lower = input.trim().to_ascii_lowercase();
    let lower = ["sha256:", "sha512:", "md5:", "sha256", "sha512", "md5"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(&lower);
    let hex: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let alg = match hex.len() {
        64 => HashAlgorithm::Sha256,
        128 => HashAlgorithm::Sha512,
        32 => HashAlgorithm::Md5,
        _ => return None,
    };
    Some((alg, hex))
}

/// Alle Hex-Werte der passenden Länge auf einer Seite (z. B. in einer HTML-Tabelle).
pub fn hashes_on_page(text: &str, algorithm: HashAlgorithm) -> Vec<String> {
    static HEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9A-Fa-f]{32,128}").expect("Regex"));
    HEX.find_iter(text)
        .map(|m| m.as_str())
        // Nur exakte Länge – kein Teilstück eines längeren Werts.
        .filter(|h| h.len() == hex_len(algorithm))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// Eine vorhandene Datei prüfen. Ergebnis wie nach einem Download – nur damit darf geflasht werden.
pub async fn verify_local<F: Fetcher>(
    f: &F,
    path: &Path,
    expected: &Expected,
    cancel: &AtomicBool,
    mut on_event: impl FnMut(AcquireEvent) + Send,
) -> Result<Acquired, AcquireError> {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    safe_filename(&filename)?;
    let algorithm = expected.algorithm();

    // Zuerst die Liste holen – schlägt das fehl, muss man nicht erst Gigabytes hashen.
    let allowed: Vec<String> = match expected {
        Expected::Hash { value, .. } => vec![value.to_ascii_lowercase()],
        Expected::PageListing { url, algorithm } => {
            on_event(AcquireEvent::Stage {
                stage: Stage::Checksum,
            });
            let page = f.get_text(url).await?;
            let list = hashes_on_page(&page, *algorithm);
            if list.is_empty() {
                return Err(AcquireError::ChecksumNotListed {
                    filename: filename.clone(),
                    url: url.to_string(),
                });
            }
            list
        }
    };

    on_event(AcquireEvent::Stage {
        stage: Stage::Verify,
    });
    let total = tokio::fs::metadata(path)
        .await
        .map_err(|e| AcquireError::Download(e.into()))?
        .len();
    let mut h = Hasher::new(algorithm);
    let size = hash_file(path, &mut h, cancel, |done| {
        on_event(AcquireEvent::Progress {
            stage: Stage::Verify,
            done,
            total: Some(total),
            bytes_per_sec: 0.0,
        })
    })
    .await?;
    let actual = h.finish_hex();

    if !allowed.iter().any(|a| hashes_equal(a, &actual)) {
        return Err(match expected {
            Expected::Hash { value, .. } => AcquireError::ChecksumMismatch {
                expected: value.clone(),
                actual,
            },
            Expected::PageListing { url, .. } => AcquireError::ChecksumNotListed {
                filename,
                url: url.to_string(),
            },
        });
    }
    Ok(Acquired {
        path: path.to_owned(),
        filename,
        size,
        algorithm,
        hash: actual,
        signer: None,
        assurance: if algorithm == HashAlgorithm::Md5 {
            Assurance::Weak
        } else {
            Assurance::Checksum
        },
        from_cache: false,
    })
}

// --- Windows-Installer erkennen ----------------------------------------------------------------

const SECTOR: u64 = 2048;

/// Ist das ein Windows-Installations-ISO? Liest das ISO9660-Hauptverzeichnis (die Kompatibilitäts-
/// schicht, die auch Windows-ISOs neben UDF haben) und sucht `SOURCES/` und `BOOTMGR`.
pub fn is_windows_installer(path: &Path) -> std::io::Result<bool> {
    let mut f = File::open(path)?;
    let mut pvd = [0u8; 2048];
    f.seek(SeekFrom::Start(16 * SECTOR))?;
    if f.read_exact(&mut pvd).is_err() || pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return Ok(false); // kein ISO9660 (z. B. .img)
    }
    // Verzeichniseintrag des Hauptverzeichnisses steht ab Byte 156 des Primary Volume Descriptor.
    let root = &pvd[156..190];
    let lba = u32::from_le_bytes(root[2..6].try_into().expect("4 Bytes")) as u64;
    let len = u32::from_le_bytes(root[10..14].try_into().expect("4 Bytes")) as u64;
    if len == 0 || len > 1024 * 1024 {
        return Ok(false);
    }
    let mut dir = vec![0u8; len as usize];
    f.seek(SeekFrom::Start(lba * SECTOR))?;
    f.read_exact(&mut dir)?;

    let names = directory_names(&dir);
    let has_sources = names.iter().any(|(n, is_dir)| *is_dir && n == "SOURCES");
    let has_bootmgr = names
        .iter()
        .any(|(n, is_dir)| !*is_dir && n.starts_with("BOOTMGR"));
    Ok(has_sources && has_bootmgr)
}

/// Einträge eines ISO9660-Verzeichnisses: (Name in Großbuchstaben ohne „;1“, ist Verzeichnis).
fn directory_names(dir: &[u8]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < dir.len() {
        let rec_len = dir[pos] as usize;
        if rec_len == 0 {
            // Rest des Sektors ist leer → nächster Sektor.
            pos = (pos / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        if pos + rec_len > dir.len() || rec_len < 34 {
            break;
        }
        let rec = &dir[pos..pos + rec_len];
        let name_len = rec[32] as usize;
        if 33 + name_len <= rec.len() {
            let raw = &rec[33..33 + name_len];
            // 0x00 = „.“, 0x01 = „..“
            if raw != [0] && raw != [1] {
                let name = String::from_utf8_lossy(raw).to_ascii_uppercase();
                let name = name
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_owned();
                out.push((name, rec[25] & 0x02 != 0));
            }
        }
        pos += rec_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_hash_formats() {
        let h = "193BBDE65EC84E298A1C798959489C8375ED501CF973C6D37FA52BB22EE8D43B";
        let want = Some((HashAlgorithm::Sha256, h.to_ascii_lowercase()));
        assert_eq!(parse_user_hash(h), want);
        assert_eq!(parse_user_hash(&format!("  sha256:{h}\n")), want);
        assert_eq!(
            parse_user_hash(&format!("SHA256 {}", h.to_lowercase())),
            want
        );
        assert_eq!(
            parse_user_hash("0c365dc3c17b05a4b276c579168b01da").map(|x| x.0),
            Some(HashAlgorithm::Md5)
        );
        assert_eq!(parse_user_hash("xyz"), None);
        assert_eq!(parse_user_hash(&h[..63]), None);
    }

    #[test]
    fn page_hashes_exact_length_only() {
        let page = r#"<td>German 64-bit</td><td>193BBDE65EC84E298A1C798959489C8375ED501CF973C6D37FA52BB22EE8D43B</td>
                      <td>abc</td><td>{}</td>"#
            .replace("{}", &"a".repeat(128));
        let found = hashes_on_page(&page, HashAlgorithm::Sha256);
        assert_eq!(
            found.len(),
            1,
            "der 128-stellige Wert darf nicht als SHA-256 durchgehen"
        );
        assert!(found[0].starts_with("193bbde6"));
    }
}
