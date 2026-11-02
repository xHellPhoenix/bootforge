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
use crate::catalog::{Compression, FlashMode, HashAlgorithm};
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

// --- Was ist das für eine Datei? -----------------------------------------------------------------

/// Ergebnis von [`inspect`]: wie die Datei auf den Stick kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub flash_mode: FlashMode,
    pub compression: Compression,
    /// Hat das Image einen Bootsektor (MBR/GPT-Schutz-MBR)? Linux-ISOs sind „Hybrid-ISOs“ mit
    /// einem solchen Sektor und starten 1:1 geschrieben. Fehlt er, startet der Stick
    /// wahrscheinlich nicht → Warnung in der GUI. Bei komprimierten Images unbekannt (`None`).
    pub boot_record: Option<bool>,
}

/// Komprimierung an der Dateiendung erkennen (`.img.xz` usw.).
pub fn compression_from_name(name: &str) -> Compression {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".xz") {
        Compression::Xz
    } else if lower.ends_with(".gz") {
        Compression::Gzip
    } else if lower.ends_with(".zst") {
        Compression::Zstd
    } else {
        Compression::None
    }
}

pub fn inspect(path: &Path) -> std::io::Result<ImageInfo> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let compression = compression_from_name(&name);
    if !compression.is_none() {
        return Ok(ImageInfo {
            flash_mode: FlashMode::Raw,
            compression,
            boot_record: None,
        });
    }
    let flash_mode = if is_windows_installer(path)? {
        FlashMode::Windows
    } else {
        FlashMode::Raw
    };
    let mut sector = [0u8; 512];
    let mut f = File::open(path)?;
    let boot_record = f.read_exact(&mut sector).is_ok() && sector[510..512] == [0x55, 0xAA];
    Ok(ImageInfo {
        flash_mode,
        compression,
        boot_record: Some(boot_record),
    })
}

// --- Windows-Installer erkennen ----------------------------------------------------------------

const SECTOR: u64 = 2048;

/// Ist das ein Windows-Installations-ISO? Gesucht wird im Hauptverzeichnis nach `sources/` und
/// `bootmgr` – zuerst im UDF-Dateisystem, dann im ISO9660-Teil.
///
/// Warum beides? Microsofts ISOs sind „UDF-Bridge“-Images: Die Dateien stehen nur im UDF-Teil,
/// der ISO9660-Teil (für sehr alte Systeme) enthält nur eine `README.TXT`. Andere Werkzeuge
/// (z. B. ältere Tools oder selbst gebaute ISOs) haben die Dateien dagegen nur in ISO9660.
pub fn is_windows_installer(path: &Path) -> std::io::Result<bool> {
    let mut f = File::open(path)?;
    let looks_like_windows = |names: &[(String, bool)]| {
        names.iter().any(|(n, is_dir)| *is_dir && n == "SOURCES")
            && names
                .iter()
                .any(|(n, is_dir)| !*is_dir && n.starts_with("BOOTMGR"))
    };
    // Kaputte oder fremde Strukturen sind kein Fehler, sondern einfach „kein Windows-ISO“.
    if let Ok(Some(names)) = udf::root_names(&mut f)
        && looks_like_windows(&names)
    {
        return Ok(true);
    }
    match iso9660_root_names(&mut f) {
        Ok(Some(names)) => Ok(looks_like_windows(&names)),
        Ok(None) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

/// Namen im ISO9660-Hauptverzeichnis (`None`: kein ISO9660).
fn iso9660_root_names(f: &mut File) -> std::io::Result<Option<Vec<(String, bool)>>> {
    let mut pvd = [0u8; 2048];
    f.seek(SeekFrom::Start(16 * SECTOR))?;
    if f.read_exact(&mut pvd).is_err() || pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return Ok(None); // kein ISO9660 (z. B. .img)
    }
    // Verzeichniseintrag des Hauptverzeichnisses steht ab Byte 156 des Primary Volume Descriptor.
    let root = &pvd[156..190];
    let lba = u32::from_le_bytes(root[2..6].try_into().expect("4 Bytes")) as u64;
    let len = u32::from_le_bytes(root[10..14].try_into().expect("4 Bytes")) as u64;
    if len == 0 || len > 1024 * 1024 {
        return Ok(None);
    }
    let mut dir = vec![0u8; len as usize];
    f.seek(SeekFrom::Start(lba * SECTOR))?;
    f.read_exact(&mut dir)?;
    Ok(Some(directory_names(&dir)))
}

/// Minimaler UDF-Leser (ECMA-167 / OSTA UDF 1.02) – gerade genug, um die Namen im
/// Hauptverzeichnis zu lesen. Weg dorthin:
///
/// ```text
/// Sektor 256: Anchor ──► Volume Descriptor Sequence
///                         ├─ Partition Descriptor   → wo die Partition beginnt
///                         └─ Logical Volume Desc.   → wo der File Set Descriptor liegt
/// File Set Descriptor ──► File Entry des Hauptverzeichnisses ──► Verzeichnisdaten (FIDs)
/// ```
mod udf {
    use std::fs::File;
    use std::io::{self, Read, Seek, SeekFrom};

    const BLOCK: u64 = 2048;
    const MAX_DIR: u64 = 4 * 1024 * 1024;

    fn u16le(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn u32le(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().expect("4 Bytes"))
    }
    fn u64le(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().expect("8 Bytes"))
    }

    fn block(f: &mut File, n: u64) -> io::Result<[u8; BLOCK as usize]> {
        let mut b = [0u8; BLOCK as usize];
        f.seek(SeekFrom::Start(n * BLOCK))?;
        f.read_exact(&mut b)?;
        Ok(b)
    }

    /// Kennung („Tag Identifier“) eines Deskriptors, nur wenn die Prüfsumme des Tags stimmt.
    fn tag(b: &[u8]) -> Option<u16> {
        let sum = b[..16]
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 4)
            .fold(0u8, |a, (_, &x)| a.wrapping_add(x));
        (sum == b[4]).then(|| u16le(b, 0))
    }

    fn bad() -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, "UDF-Struktur unerwartet")
    }

    /// `None`: kein (lesbares) UDF.
    pub fn root_names(f: &mut File) -> io::Result<Option<Vec<(String, bool)>>> {
        let anchor = block(f, 256)?;
        if tag(&anchor) != Some(2) {
            return Ok(None);
        }
        let vds_len = u32le(&anchor, 16) as u64;
        let vds_loc = u32le(&anchor, 20) as u64;

        let (mut part_start, mut fsd) = (None, None);
        for i in 0..(vds_len / BLOCK).min(64) {
            let d = block(f, vds_loc + i)?;
            match tag(&d) {
                Some(5) => part_start = Some(u32le(&d, 188) as u64),
                Some(6) => {
                    if u32le(&d, 212) as u64 != BLOCK {
                        return Ok(None);
                    }
                    // Nur eine einfache Partition („Type 1 Map“) – so bauen es Microsofts Werkzeuge.
                    if u32le(&d, 268) != 1 || d[440] != 1 {
                        return Ok(None);
                    }
                    fsd = Some(u32le(&d, 252) as u64);
                }
                Some(8) => break, // Terminating Descriptor
                _ => {}
            }
        }
        let (Some(part), Some(fsd)) = (part_start, fsd) else {
            return Ok(None);
        };

        let fs = block(f, part + fsd)?;
        if tag(&fs) != Some(256) {
            return Ok(None);
        }
        let root_icb = u32le(&fs, 404) as u64;
        let dir = read_file(f, part, root_icb)?;
        Ok(Some(parse_fids(&dir)?))
    }

    /// Inhalt einer (kleinen) Datei bzw. eines Verzeichnisses über ihren File Entry lesen.
    fn read_file(f: &mut File, part: u64, icb: u64) -> io::Result<Vec<u8>> {
        let fe = block(f, part + icb)?;
        let (ad_base, len_ea, len_ad) = match tag(&fe) {
            Some(261) => (176usize, u32le(&fe, 168) as usize, u32le(&fe, 172) as usize),
            Some(266) => (216usize, u32le(&fe, 208) as usize, u32le(&fe, 212) as usize),
            _ => return Err(bad()),
        };
        let size = u64le(&fe, 56);
        if size > MAX_DIR {
            return Err(bad());
        }
        let ads_at = ad_base + len_ea;
        let ads = fe.get(ads_at..ads_at + len_ad).ok_or_else(bad)?;
        let mut out = Vec::with_capacity(size as usize);
        match u16le(&fe, 34) & 7 {
            // Daten direkt im File Entry
            3 => out.extend_from_slice(ads.get(..size as usize).ok_or_else(bad)?),
            // short_ad (8 Bytes) bzw. long_ad (16 Bytes): Länge + Block
            kind @ (0 | 1) => {
                let step = if kind == 0 { 8 } else { 16 };
                for ad in ads.chunks_exact(step) {
                    let len = (u32le(ad, 0) & 0x3FFF_FFFF) as u64;
                    if len == 0 {
                        break;
                    }
                    let mut buf = vec![0u8; len.min(MAX_DIR) as usize];
                    f.seek(SeekFrom::Start((part + u32le(ad, 4) as u64) * BLOCK))?;
                    f.read_exact(&mut buf)?;
                    out.extend_from_slice(&buf);
                    if out.len() as u64 >= size {
                        break;
                    }
                }
                out.truncate(size as usize);
            }
            _ => return Err(bad()),
        }
        Ok(out)
    }

    /// File Identifier Descriptors (Verzeichniseinträge) → (Name groß, ist Verzeichnis).
    fn parse_fids(dir: &[u8]) -> io::Result<Vec<(String, bool)>> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos + 38 <= dir.len() {
            let d = &dir[pos..];
            if u16le(d, 0) != 257 {
                return Err(bad());
            }
            let chars = d[18];
            let l_fi = d[19] as usize;
            let l_iu = u16le(d, 36) as usize;
            let name_at = 38 + l_iu;
            let name = d.get(name_at..name_at + l_fi).ok_or_else(bad)?;
            // Bit 3: Verweis auf das übergeordnete Verzeichnis („..“), Bit 2: gelöscht
            if chars & 0x0C == 0 && !name.is_empty() {
                out.push((decode(name).to_uppercase(), chars & 0x02 != 0));
            }
            pos += (38 + l_iu + l_fi + 3) & !3;
        }
        Ok(out)
    }

    /// OSTA „Compressed Unicode“: 1. Byte 8 = ein Byte je Zeichen, 16 = UTF-16 (Big Endian).
    fn decode(b: &[u8]) -> String {
        match b[0] {
            8 => b[1..].iter().map(|&c| c as char).collect(),
            16 => char::decode_utf16(
                b[1..]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]])),
            )
            .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect(),
            _ => String::new(),
        }
    }
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
