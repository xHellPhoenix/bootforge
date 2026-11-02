//! Links aus HTML-Verzeichnislistings herausziehen.
//!
//! Warum keine HTML-Bibliothek? Verzeichnislistings (Apache, nginx, MirrorCache, S3-Indizes)
//! sind maschinell erzeugt und sehr einfach aufgebaut; uns interessieren nur `href`-Attribute.
//! Eine Regex reicht dafür und spart eine große Abhängigkeit. Für „echte“ Webseiten mit
//! JavaScript wäre das nicht robust genug – dafür gibt es die Quellen `json_api`/`github_release`.

use std::sync::LazyLock;

use percent_encoding::percent_decode_str;
use regex::Regex;
use url::Url;

static HREF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)href\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("gültige Regex")
});

/// Liefert alle Links eines Listings **relativ zu `base`**, normalisiert:
///
/// - Query (`?C=M;O=A`) und Fragment (`#x`) entfernt,
/// - führendes `./` entfernt,
/// - absolute Links (`/pub/…`, `https://…`) nur, wenn sie *unterhalb* von `base` liegen,
/// - URL-dekodiert (`%2B` → `+`),
/// - ohne `../`, ohne Duplikate, Reihenfolge wie im Dokument.
pub fn extract_links(html: &str, base: &Url) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cap in HREF.captures_iter(html) {
        let raw = cap
            .get(1)
            .or_else(|| cap.get(2))
            .map(|m| m.as_str())
            .unwrap_or("");
        let raw = raw.split(['?', '#']).next().unwrap_or("").trim();
        if raw.is_empty() || raw.starts_with("mailto:") || raw.starts_with("javascript:") {
            continue;
        }

        let relative = if raw.starts_with('/') || raw.contains("://") {
            // Absolut → in Bezug auf base setzen.
            let Ok(abs) = base.join(raw) else { continue };
            match abs.as_str().strip_prefix(base.as_str()) {
                Some(rest) if abs.host() == base.host() => rest.to_owned(),
                _ => continue,
            }
        } else {
            raw.trim_start_matches("./").to_owned()
        };

        let decoded = percent_decode_str(&relative)
            .decode_utf8_lossy()
            .into_owned();
        if decoded.is_empty() || decoded.starts_with("..") || decoded.starts_with('/') {
            continue;
        }
        if !out.contains(&decoded) {
            out.push(decoded);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    #[test]
    fn apache_listing() {
        let html = r#"<a href="?C=N;O=D">Name</a> <a href="/pub/">Parent Directory</a>
            <a href="43/">43/</a> <a href="44/">44/</a> <a href="test/">test/</a>"#;
        let links = extract_links(html, &base("https://dl.example.org/pub/releases/"));
        assert_eq!(links, ["43/", "44/", "test/"]);
    }

    #[test]
    fn dot_slash_and_mirrorcache() {
        let html = r#"<a href='./openSUSE-DVD.iso'>x</a><a href="./openSUSE-DVD.iso.mirrorlist">m</a>
            <a href="./16.0/">16.0</a><a href="../">up</a>"#;
        let links = extract_links(html, &base("https://download.opensuse.org/x/"));
        assert_eq!(
            links,
            ["openSUSE-DVD.iso", "openSUSE-DVD.iso.mirrorlist", "16.0/"]
        );
    }

    #[test]
    fn absolute_links_only_below_base() {
        let html = r#"<a href="https://other.example/a.iso">a</a>
            <a href="https://mirror.example/iso/latest/arch.iso">b</a>
            <a href="/iso/latest/arch.iso.sig">c</a>
            <a href="/iso/">up</a>"#;
        let links = extract_links(html, &base("https://mirror.example/iso/latest/"));
        assert_eq!(links, ["arch.iso", "arch.iso.sig"]);
    }

    #[test]
    fn percent_decoding_and_duplicates() {
        let html =
            r#"<a href="file%2B1.iso">x</a><a href="file+1.iso">y</a><a href="mailto:a@b">z</a>"#;
        assert_eq!(extract_links(html, &base("https://e.org/")), ["file+1.iso"]);
    }
}
