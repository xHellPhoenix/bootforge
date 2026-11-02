//! Prüfsummendateien lesen und Hashes berechnen.
//!
//! Unterstützte Formate (alle in echten Distros gesehen):
//!
//! ```text
//! GNU:        487f…dd2 *ubuntu-26.04-desktop-amd64.iso     (Ubuntu, Mint: `*` = Binärmodus)
//!             4e88…c6c  proxmox-ve_9.2-1.iso                (Proxmox, openSUSE, Arch)
//! BSD:        SHA256 (Fedora-…x86_64.iso) = 1620…ddf        (Fedora)
//! nur Hash:   54ce…c8e                                      (TrueNAS, Batocera)
//! ```

use md5::Md5;
use sha2::{Digest, Sha256, Sha512};

use crate::catalog::HashAlgorithm;

/// Hex-Länge je Algorithmus.
pub fn hex_len(alg: HashAlgorithm) -> usize {
    match alg {
        HashAlgorithm::Sha256 => 64,
        HashAlgorithm::Sha512 => 128,
        HashAlgorithm::Md5 => 32,
    }
}

fn is_hash(s: &str, alg: HashAlgorithm) -> bool {
    s.len() == hex_len(alg) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Sucht in einer Prüfsummendatei den Hash für `filename`.
///
/// Gibt den Hash in Kleinbuchstaben zurück. **Mehrdeutigkeit ist ein Fehler**: Stehen für dieselbe
/// Datei zwei verschiedene Hashes drin, liefern wir `None` statt zu raten.
pub fn find_hash(text: &str, filename: &str, alg: HashAlgorithm) -> Option<String> {
    let text = strip_clearsign_frame(text);
    let mut found: Option<String> = None;
    let mut only_hashes = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let hit = parse_bsd(line, filename, alg).or_else(|| parse_gnu(line, filename, alg));
        if let Some(h) = hit {
            let h = h.to_ascii_lowercase();
            match &found {
                Some(prev) if *prev != h => return None,
                _ => found = Some(h),
            }
        } else if is_hash(line, alg) {
            only_hashes.push(line.to_ascii_lowercase());
        }
    }

    // Datei enthält nur einen nackten Hash (Sidecar-Datei wie `x.iso.sha256`).
    found.or_else(|| match only_hashes.as_slice() {
        [single] => Some(single.clone()),
        _ => None,
    })
}

/// `SHA256 (datei) = hash`
fn parse_bsd<'a>(line: &'a str, filename: &str, alg: HashAlgorithm) -> Option<&'a str> {
    let (head, hash) = line.rsplit_once(" = ")?;
    let (algo, rest) = head.split_once(" (")?;
    let name = rest.strip_suffix(')')?;
    let algo_ok = matches!(
        (algo.to_ascii_uppercase().as_str(), alg),
        ("SHA256", HashAlgorithm::Sha256)
            | ("SHA512", HashAlgorithm::Sha512)
            | ("MD5", HashAlgorithm::Md5)
    );
    (algo_ok && same_name(name, filename) && is_hash(hash.trim(), alg)).then(|| hash.trim())
}

/// `hash  datei` bzw. `hash *datei`
fn parse_gnu<'a>(line: &'a str, filename: &str, alg: HashAlgorithm) -> Option<&'a str> {
    let (hash, rest) = line.split_once(char::is_whitespace)?;
    let name = rest.trim_start().trim_start_matches('*');
    (is_hash(hash, alg) && same_name(name, filename)).then_some(hash)
}

fn same_name(listed: &str, wanted: &str) -> bool {
    let listed = listed.trim().trim_start_matches("./");
    // Manche Dateien listen Pfade (`iso/x.iso`) – der Dateiname am Ende zählt.
    listed == wanted || listed.rsplit('/').next() == Some(wanted)
}

/// Entfernt den Rahmen einer PGP-Clearsign-Nachricht und liefert nur den signierten Text.
///
/// **Achtung:** Das prüft die Signatur NICHT. Für signierte Dateien nimmt der Aufrufer den Text,
/// den die GPG-Prüfung als verifiziert zurückgibt – sonst könnte ein Angreifer Zeilen außerhalb
/// des signierten Bereichs einschmuggeln.
pub fn strip_clearsign_frame(text: &str) -> std::borrow::Cow<'_, str> {
    const BEGIN: &str = "-----BEGIN PGP SIGNED MESSAGE-----";
    const SIG: &str = "-----BEGIN PGP SIGNATURE-----";
    let Some(start) = text.find(BEGIN) else {
        return text.into();
    };
    let body = &text[start + BEGIN.len()..];
    // Header (`Hash: SHA256`) endet mit einer Leerzeile.
    let body = body
        .split_once("\n\n")
        .or_else(|| body.split_once("\r\n\r\n"))
        .map_or("", |(_, b)| b);
    let body = body.split(SIG).next().unwrap_or("");
    // Dash-Escaping rückgängig machen (`- -----` → `-----`).
    body.lines()
        .map(|l| l.strip_prefix("- ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
        .into()
}

/// Inkrementeller Hasher für alle unterstützten Algorithmen.
#[derive(Clone)]
pub enum Hasher {
    Sha256(Sha256),
    Sha512(Sha512),
    Md5(Md5),
}

impl Hasher {
    pub fn new(alg: HashAlgorithm) -> Self {
        match alg {
            HashAlgorithm::Sha256 => Hasher::Sha256(Sha256::new()),
            HashAlgorithm::Sha512 => Hasher::Sha512(Sha512::new()),
            HashAlgorithm::Md5 => Hasher::Md5(Md5::new()),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha256(h) => h.update(data),
            Hasher::Sha512(h) => h.update(data),
            Hasher::Md5(h) => h.update(data),
        }
    }

    /// Ergebnis als Hex in Kleinbuchstaben.
    pub fn finish_hex(self) -> String {
        let bytes: Vec<u8> = match self {
            Hasher::Sha256(h) => h.finalize().to_vec(),
            Hasher::Sha512(h) => h.finalize().to_vec(),
            Hasher::Md5(h) => h.finalize().to_vec(),
        };
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// Vergleich in konstanter Zeit. Bei Prüfsummen eher Gewohnheit als Notwendigkeit, aber
/// es schadet nicht und ist bei sicherheitsrelevanten Vergleichen gute Praxis.
pub fn hashes_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len()
        && a.iter().zip(b).fold(0u8, |acc, (x, y)| {
            acc | (x.to_ascii_lowercase() ^ y.to_ascii_lowercase())
        }) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use HashAlgorithm::*;

    const UBUNTU: &str = include_str!("../../tests/fixtures/ubuntu-SHA256SUMS");
    const FEDORA: &str = include_str!("../../tests/fixtures/fedora-CHECKSUM");
    const MINT: &str = include_str!("../../tests/fixtures/mint-sha256sum.txt");
    const TRUENAS: &str = include_str!("../../tests/fixtures/truenas.sha256");

    #[test]
    fn gnu_binary_marker() {
        assert_eq!(
            find_hash(UBUNTU, "ubuntu-26.04-desktop-amd64.iso", Sha256).as_deref(),
            Some("487f87faaf547ea30e0aba4d5b53346292571256b25333a978db1692bcee9dd2")
        );
        assert_eq!(
            find_hash(MINT, "linuxmint-22.3-xfce-64bit.iso", Sha256).as_deref(),
            Some("45a835b5dddaf40e84d776549e0b19b3fbd49673b6cc6434ebddbfcd217df776")
        );
    }

    #[test]
    fn bsd_in_clearsigned_file() {
        assert_eq!(
            find_hash(FEDORA, "Fedora-Workstation-Live-44-1.7.x86_64.iso", Sha256).as_deref(),
            Some("1620295f6a00c27c3208f0c00b8ece4eab1ec69b9002152d97488bf26a426ddf")
        );
    }

    #[test]
    fn bare_hash_sidecar() {
        assert_eq!(
            find_hash(TRUENAS, "TrueNAS-SCALE-25.10.7.iso", Sha256).map(|h| h.len()),
            Some(64)
        );
        assert_eq!(
            find_hash("0c365dc3c17b05a4b276c579168b01da\n", "x.img.gz", Md5).as_deref(),
            Some("0c365dc3c17b05a4b276c579168b01da")
        );
    }

    #[test]
    fn wrong_file_or_algorithm_finds_nothing() {
        assert_eq!(
            find_hash(UBUNTU, "ubuntu-99.04-desktop-amd64.iso", Sha256),
            None
        );
        assert_eq!(
            find_hash(UBUNTU, "ubuntu-26.04-desktop-amd64.iso", Sha512),
            None
        );
        // Nur-Hash-Fallback greift nicht bei Dateien mit mehreren Einträgen.
        assert_eq!(find_hash("aa\nbb\n", "x", Md5), None);
    }

    #[test]
    fn conflicting_entries_are_rejected() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let text = format!("{a}  x.iso\n{b}  x.iso\n");
        assert_eq!(find_hash(&text, "x.iso", Sha256), None);
    }

    #[test]
    fn no_prefix_matches() {
        // `x.iso` darf nicht auf `x.iso.zsync` oder `my-x.iso` passen.
        let h = "c".repeat(64);
        let text = format!("{h}  x.iso.zsync\n{h}  my-x.iso\n");
        assert_eq!(find_hash(&text, "x.iso", Sha256), None);
    }

    #[test]
    fn hasher_known_values() {
        let mut h = Hasher::new(Sha256);
        h.update(b"abc");
        assert_eq!(
            h.finish_hex(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let mut h = Hasher::new(Md5);
        h.update(b"abc");
        assert_eq!(h.finish_hex(), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn constant_time_compare() {
        assert!(hashes_equal("ABCdef", "abcDEF"));
        assert!(!hashes_equal("abc", "abd"));
        assert!(!hashes_equal("abc", "abcd"));
    }
}
