//! Inhaltliche Prüfung eines Katalogs.
//!
//! `serde` prüft nur die *Form* (richtige Felder, richtige Typen). Hier prüfen wir die
//! *Bedeutung*: eindeutige IDs, nur HTTPS, gültige Regexe, plausible Fingerprints …
//! Ein Katalog mit Fehlern wird von der App komplett abgelehnt (dann greift der Cache/Fallback).

use std::collections::HashSet;
use std::fmt;

use regex::Regex;
use serde::Serialize;

use super::model::*;

/// Schweregrad eines Befunds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Katalog ist unbrauchbar/unsicher → wird abgelehnt.
    Error,
    /// Erlaubt, aber auffällig (z. B. nur MD5).
    Warning,
}

/// Ein einzelner Befund.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    /// Wo im Katalog, z. B. `ubuntu/ubuntu-desktop-lts/source`.
    pub location: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self.severity {
            Severity::Error => "Fehler",
            Severity::Warning => "Warnung",
        };
        write!(f, "[{s}] {}: {}", self.location, self.message)
    }
}

/// Prüft den Katalog und liefert alle Befunde (leer = alles gut).
pub fn validate(catalog: &Catalog) -> Vec<Issue> {
    let mut v = Validator::default();
    v.catalog(catalog);
    v.issues
}

#[derive(Default)]
struct Validator {
    issues: Vec<Issue>,
}

impl Validator {
    fn err(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            location: loc.into(),
            message: msg.into(),
        });
    }
    fn warn(&mut self, loc: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Warning,
            location: loc.into(),
            message: msg.into(),
        });
    }

    fn catalog(&mut self, c: &Catalog) {
        if c.schema_version != SCHEMA_VERSION {
            self.err(
                "schema_version",
                format!(
                    "Katalogformat {} wird nicht unterstützt (erwartet {SCHEMA_VERSION})",
                    c.schema_version
                ),
            );
        }

        let mut category_ids = HashSet::new();
        for cat in &c.categories {
            self.id(&format!("categories/{}", cat.id), &cat.id);
            if !category_ids.insert(cat.id.as_str()) {
                self.err(format!("categories/{}", cat.id), "Kategorie-ID doppelt");
            }
            self.text(&format!("categories/{}/name", cat.id), &cat.name);
        }

        let mut distro_ids = HashSet::new();
        let mut variant_ids = HashSet::new();
        for d in &c.distros {
            let loc = d.id.clone();
            self.id(&loc, &d.id);
            if !distro_ids.insert(d.id.as_str()) {
                self.err(&loc, "Distro-ID doppelt");
            }
            self.https(&format!("{loc}/homepage"), &d.homepage);
            self.text(&format!("{loc}/description"), &d.description);
            if d.variants.is_empty() {
                self.err(&loc, "Distro hat keine Varianten");
            }
            for var in &d.variants {
                let vloc = format!("{loc}/{}", var.id);
                self.id(&vloc, &var.id);
                if !variant_ids.insert(var.id.as_str()) {
                    self.err(
                        &vloc,
                        "Varianten-ID doppelt (muss katalogweit eindeutig sein)",
                    );
                }
                for cat in &var.categories {
                    if !category_ids.contains(cat.as_str()) {
                        self.err(&vloc, format!("unbekannte Kategorie `{cat}`"));
                    }
                }
                if var.categories.is_empty() {
                    self.warn(&vloc, "Variante ist keiner Kategorie zugeordnet");
                }
                self.variant(&vloc, var);
            }
        }
    }

    fn variant(&mut self, loc: &str, v: &Variant) {
        self.text(&format!("{loc}/name"), &v.name);
        self.text(&format!("{loc}/description"), &v.description);
        if let Some(n) = &v.notes {
            self.text(&format!("{loc}/notes"), n);
        }

        // Quelle
        let sloc = format!("{loc}/source");
        match &v.source {
            Source::DirectoryListing { base_url, steps } => {
                self.https(&sloc, base_url);
                if steps.is_empty() {
                    self.err(&sloc, "`steps` darf nicht leer sein");
                }
                for (i, step) in steps.iter().enumerate() {
                    match step {
                        Step::Path { path } => {
                            if path.contains("://")
                                || path.starts_with('/')
                                || path.split('/').any(|p| p == "..")
                            {
                                self.err(
                                    format!("{sloc}/steps/{i}"),
                                    "`path` muss ein relativer Unterpfad sein",
                                );
                            }
                        }
                        Step::Match { pattern, .. } => {
                            self.regex(&format!("{sloc}/steps/{i}"), pattern)
                        }
                    }
                }
                if !matches!(steps.last(), Some(Step::Match { .. })) {
                    self.err(
                        &sloc,
                        "der letzte Schritt muss ein `match` auf die Image-Datei sein",
                    );
                }
            }
            Source::JsonApi { url, fields } => {
                self.https(&sloc, url);
                for (name, p) in [
                    ("url", Some(&fields.url)),
                    ("version", Some(&fields.version)),
                ]
                .into_iter()
                .chain([
                    ("build", fields.build.as_ref()),
                    ("checksum", fields.checksum.as_ref()),
                    ("size", fields.size.as_ref()),
                ]) {
                    if let Some(p) = p {
                        if !p.starts_with('/') {
                            self.err(
                                format!("{sloc}/fields/{name}"),
                                "JSON-Pointer muss mit `/` beginnen",
                            );
                        }
                    }
                }
            }
            Source::GithubRelease {
                repo,
                asset_pattern,
                ..
            } => {
                let ok = repo.split('/').count() == 2 && repo.split('/').all(|p| !p.is_empty());
                if !ok {
                    self.err(&sloc, "`repo` muss die Form `besitzer/repo` haben");
                }
                self.regex(&sloc, asset_pattern);
            }
            Source::Microsoft { .. } => {
                if v.flash_mode != FlashMode::Windows {
                    self.err(&sloc, "Microsoft-Quellen brauchen `flash_mode: windows`");
                }
            }
        }

        if v.flash_mode == FlashMode::Windows && !v.compression.is_none() {
            self.err(loc, "`flash_mode: windows` unterstützt keine Komprimierung");
        }

        // Prüfsumme
        let cloc = format!("{loc}/checksum");
        let algorithm = match &v.checksum {
            Checksum::File {
                url,
                pattern,
                algorithm,
            } => {
                match (url, pattern) {
                    (Some(u), None) => self.url_or_relative(&cloc, u),
                    (None, Some(p)) => {
                        self.regex(&cloc, p);
                        if !matches!(v.source, Source::DirectoryListing { .. }) {
                            self.err(&cloc, "`match` geht nur bei `directory_listing`-Quellen");
                        }
                    }
                    _ => self.err(&cloc, "genau eins von `url` oder `match` angeben"),
                }
                *algorithm
            }
            Checksum::FromSource { algorithm } => {
                let provides = match &v.source {
                    Source::JsonApi { fields, .. } => fields.checksum.is_some(),
                    Source::Microsoft { .. } => true,
                    _ => false,
                };
                if !provides {
                    self.err(
                        &cloc,
                        "`from_source` braucht eine Quelle, die eine Prüfsumme liefert",
                    );
                }
                *algorithm
            }
        };
        if algorithm == HashAlgorithm::Md5 {
            self.warn(
                &cloc,
                "MD5 schützt nur vor Übertragungsfehlern, nicht vor Manipulation",
            );
        }

        // Signatur
        if let Some(sig) = &v.signature {
            let gloc = format!("{loc}/signature");
            match (sig.kind, &sig.url) {
                (SignatureKind::GpgDetached, None) => {
                    self.err(&gloc, "`gpg_detached` braucht eine `url`")
                }
                (SignatureKind::GpgClearsigned, Some(_)) => self.err(
                    &gloc,
                    "`gpg_clearsigned` hat keine eigene `url` (die Prüfsummendatei ist signiert)",
                ),
                (_, Some(u)) => self.url_or_relative(&gloc, u),
                _ => {}
            }
            if sig.kind == SignatureKind::GpgClearsigned && sig.signs != SignedTarget::ChecksumFile
            {
                self.err(
                    &gloc,
                    "`gpg_clearsigned` kann nur die Prüfsummendatei abdecken",
                );
            }
            if sig.signs == SignedTarget::ChecksumFile
                && !matches!(v.checksum, Checksum::File { .. })
            {
                self.err(&gloc, "`signs: checksum_file` ohne Prüfsummendatei");
            }
            if sig.fingerprints.is_empty() {
                self.err(&gloc, "mindestens ein Fingerprint nötig");
            }
            for fp in &sig.fingerprints {
                if fp.len() != 40
                    || !fp
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
                {
                    self.err(&gloc, format!("ungültiger Fingerprint `{fp}` (40 Hex-Zeichen, Großbuchstaben, ohne Leerzeichen)"));
                }
            }
            if let Some(k) = &sig.keyring_url {
                self.https(&gloc, k);
            }
            if sig.allow_v3 {
                self.warn(
                    &gloc,
                    "veraltete v3-Signaturen sind erlaubt (Ausnahme, siehe FORMAT.md)",
                );
            }
        }
    }

    fn id(&mut self, loc: &str, id: &str) {
        let ok = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !id.starts_with('-');
        if !ok {
            self.err(loc, format!("ungültige ID `{id}` (nur a-z, 0-9 und `-`)"));
        }
    }

    fn text(&mut self, loc: &str, t: &LocalizedText) {
        match t.0.get("de") {
            Some(s) if !s.trim().is_empty() => {}
            _ => self.err(loc, "deutscher Text (`de`) fehlt"),
        }
    }

    fn regex(&mut self, loc: &str, pattern: &str) {
        if let Err(e) = Regex::new(pattern) {
            self.err(loc, format!("ungültige Regex `{pattern}`: {e}"));
        }
    }

    /// Absolute URL → muss HTTPS sein. Relative URL (z. B. `SHA256SUMS`) ist ok.
    fn url_or_relative(&mut self, loc: &str, u: &str) {
        if u.contains("://") || u.starts_with("//") {
            self.https(loc, u);
        }
    }

    fn https(&mut self, loc: &str, u: &str) {
        match url::Url::parse(u) {
            Ok(parsed) if parsed.scheme() == "https" && parsed.host().is_some() => {}
            Ok(_) => self.err(loc, format!("nur HTTPS erlaubt: `{u}`")),
            Err(e) => self.err(loc, format!("ungültige URL `{u}`: {e}")),
        }
    }
}
