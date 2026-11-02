//! Datenmodell des Distro-Katalogs.
//!
//! Diese Structs sind die *einzige* Quelle der Wahrheit für das Katalogformat:
//! - `serde` liest/schreibt daraus JSON,
//! - `schemars` erzeugt daraus `catalog/catalog.schema.json` (für Editor-Autovervollständigung),
//! - ein Test stellt sicher, dass die Schema-Datei nie vom Code abweicht.
//!
//! Die `///`-Kommentare landen als `description` im JSON-Schema – sie sind also
//! gleichzeitig die Doku für alle, die den Katalog pflegen.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Version des Katalogformats, die diese App versteht.
/// Wird das Format inkompatibel geändert, wird diese Zahl erhöht.
pub const SCHEMA_VERSION: u32 = 1;

/// Der komplette Katalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Optionaler Verweis auf das JSON-Schema (nur für Editoren, wird von der App ignoriert).
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Version des Katalogformats (aktuell 1).
    pub schema_version: u32,
    /// Datum der letzten Änderung im Format JJJJ-MM-TT.
    pub updated: String,
    /// Kategorien für die Filter in der GUI.
    pub categories: Vec<Category>,
    /// Alle Distributionen.
    pub distros: Vec<Distro>,
}

/// Übersetzter Text: Sprachcode (`de`, `en`, …) → Text. `de` ist Pflicht.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct LocalizedText(pub BTreeMap<String, String>);

impl LocalizedText {
    /// Text in der gewünschten Sprache, sonst Deutsch, sonst irgendeine vorhandene Sprache.
    pub fn get(&self, lang: &str) -> &str {
        self.0
            .get(lang)
            .or_else(|| self.0.get("de"))
            .or_else(|| self.0.values().next())
            .map(String::as_str)
            .unwrap_or("")
    }
}

/// Eine Kategorie (Filter-Chip in der GUI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Category {
    /// Eindeutige ID, z. B. `desktop`.
    pub id: String,
    /// Anzeigename.
    pub name: LocalizedText,
}

/// Eine Distribution, z. B. „Ubuntu“. Enthält eine oder mehrere Varianten.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Distro {
    /// Eindeutige ID, z. B. `ubuntu`.
    pub id: String,
    /// Anzeigename (wird nicht übersetzt).
    pub name: String,
    /// Herausgeber, z. B. `Canonical`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// Offizielle Website (HTTPS).
    pub homepage: String,
    /// Kurzbeschreibung.
    pub description: LocalizedText,
    /// Varianten (Desktop, Server, Netinst, …).
    pub variants: Vec<Variant>,
}

/// Eine konkrete, herunterladbare Variante einer Distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    /// Katalogweit eindeutige ID, z. B. `ubuntu-desktop-lts`.
    pub id: String,
    /// Anzeigename, z. B. „Desktop (LTS)“.
    pub name: LocalizedText,
    /// Beschreibung für die GUI.
    pub description: LocalizedText,
    /// IDs der Kategorien, in denen die Variante erscheint.
    pub categories: Vec<String>,
    /// CPU-Architektur des Images.
    pub arch: Arch,
    /// Wie das Image auf den Stick kommt.
    pub flash_mode: FlashMode,
    /// Ist das heruntergeladene Image komprimiert? (z. B. Batocera: `.img.gz`)
    #[serde(default, skip_serializing_if = "Compression::is_none")]
    pub compression: Compression,
    /// Empfohlene Mindestgröße des USB-Sticks in GB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_usb_gb: Option<u32>,
    /// Wie die neueste Version gefunden wird.
    pub source: Source,
    /// Woher die Prüfsumme kommt. Pflicht: ohne Prüfsumme wird nie geflasht.
    pub checksum: Checksum,
    /// Optionale GPG-Signatur (über die Prüfsummen-Datei oder das Image selbst).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Signature>,
    /// Optionaler Hinweis, der in der GUI angezeigt wird.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<LocalizedText>,
}

/// CPU-Architektur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// Flash-Modus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlashMode {
    /// Image 1:1 auf den Stick schreiben (Linux-Hybrid-ISOs, `.img`). Danach Zurücklesen + Vergleichen.
    Raw,
    /// Windows-Installationsmedium: GPT + FAT32 anlegen, Dateien kopieren, `install.wim` ggf. aufteilen.
    Windows,
}

/// Komprimierung des heruntergeladenen Images. Wird beim Flashen „im Fluss“ entpackt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    #[default]
    None,
    Gzip,
    Xz,
    Zstd,
}

impl Compression {
    pub fn is_none(&self) -> bool {
        matches!(self, Compression::None)
    }
}

/// Strategie, mit der die neueste Version ermittelt wird.
///
/// Alle URLs müssen HTTPS sein.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    /// HTML-Verzeichnislisting(s) durchsuchen. Ausgehend von `base_url` wird jeder Schritt
    /// nacheinander angewendet; der letzte Schritt muss die Image-Datei finden.
    ///
    /// Vor dem Vergleich werden Links normalisiert: Query/Fragment entfernt, führendes `./`
    /// entfernt, URL-dekodiert. Verzeichnisse behalten ihren abschließenden `/`.
    DirectoryListing {
        /// Start-URL (HTTPS, sollte mit `/` enden).
        base_url: String,
        /// Navigationsschritte.
        steps: Vec<Step>,
    },
    /// Eine JSON-API abfragen (z. B. Pop!_OS). Felder werden per JSON-Pointer (RFC 6901) gelesen.
    JsonApi {
        /// API-URL (HTTPS).
        url: String,
        /// Wo in der Antwort welche Information steht.
        fields: JsonFields,
    },
    /// Neuestes Release eines GitHub-Repos.
    GithubRelease {
        /// `besitzer/repo`
        repo: String,
        /// Regex für den Namen des Release-Assets.
        asset_pattern: String,
        /// Auch Vorabversionen berücksichtigen?
        #[serde(default)]
        include_prereleases: bool,
    },
    /// Der Nutzer lädt selbst herunter (z. B. Windows: Microsoft erlaubt keine automatisierten
    /// Downloads). BootForge öffnet die Seite im Browser; danach wählt man die Datei aus und
    /// BootForge prüft sie (siehe `checksum`).
    Manual {
        /// Offizielle Download-Seite (HTTPS).
        download_page: String,
        /// Regex für den erwarteten Dateinamen – nur als Hinweis bei der Auswahl.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename_pattern: Option<String>,
    },
}

/// Ein Schritt in einem Verzeichnislisting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Step {
    /// Festen Unterpfad anhängen, z. B. `Workstation/x86_64/iso/`.
    Path {
        /// Relativer Pfad.
        path: String,
    },
    /// Alle Links des aktuellen Listings gegen eine Regex prüfen und einen Treffer auswählen.
    /// Eine benannte Gruppe `(?P<version>…)` liefert die Versionsnummer.
    Match {
        /// Regex (Rust-Syntax), wird gegen den ganzen normalisierten Link geprüft – bitte mit `^…$`.
        #[serde(rename = "match")]
        pattern: String,
        /// Wie unter mehreren Treffern gewählt wird.
        #[serde(default, skip_serializing_if = "Select::is_default")]
        select: Select,
    },
}

/// Auswahlregel bei mehreren Treffern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Select {
    /// Höchste Versionsnummer (numerischer Vergleich der Zahlenblöcke: `24.10` > `24.4`).
    /// Ist der höchste Treffer eine Sackgasse (z. B. leeres Verzeichnis), wird der nächste probiert.
    #[default]
    HighestVersion,
    /// Alphabetisch letzter Treffer.
    Lexical,
}

impl Select {
    pub fn is_default(&self) -> bool {
        *self == Select::default()
    }
}

/// JSON-Pointer in eine API-Antwort, z. B. `/url`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JsonFields {
    /// Download-URL des Images.
    pub url: String,
    /// Versionsnummer.
    pub version: String,
    /// Optionale Build-Nummer (wird an die Version angehängt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    /// Prüfsumme (nötig, wenn `checksum.type` = `from_source`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// Dateigröße in Bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

/// Herkunft der Prüfsumme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Checksum {
    /// Eine Datei mit Prüfsummen herunterladen. Unterstützt werden GNU-Format
    /// (`<hash>  <datei>` bzw. `<hash> *<datei>`), BSD-Format (`SHA256 (<datei>) = <hash>`)
    /// und Dateien, die nur den Hash enthalten. PGP-Clearsign-Rahmen werden entfernt.
    ///
    /// Genau eins von `url` oder `match` angeben.
    File {
        /// URL der Datei, relativ zum Verzeichnis des Images oder absolut (HTTPS).
        /// Platzhalter: `{filename}` (Dateiname des Images), `{version}`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        /// Alternativ: Regex, die die Datei im Listing des Image-Verzeichnisses findet
        /// (z. B. Fedora: `-CHECKSUM$`).
        #[serde(rename = "match", default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        /// Hash-Algorithmus.
        algorithm: HashAlgorithm,
    },
    /// Die Quelle (JSON-API) liefert die Prüfsumme direkt mit.
    FromSource {
        /// Hash-Algorithmus.
        algorithm: HashAlgorithm,
    },
    /// Der Hash der Datei muss auf dieser HTTPS-Seite des Herstellers stehen
    /// (z. B. Microsofts Tabelle „SHA-256 je Sprache“ auf der Download-Seite).
    PageListing {
        /// Seite mit der Liste (HTTPS).
        url: String,
        /// Hash-Algorithmus.
        algorithm: HashAlgorithm,
    },
}

/// Hash-Algorithmus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
    /// Nur Schutz gegen kaputte Downloads, NICHT gegen Manipulation. Nur nutzen, wenn es nichts anderes gibt.
    Md5,
}

/// GPG-Signatur.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    /// Art der Signatur.
    #[serde(rename = "type")]
    pub kind: SignatureKind,
    /// Was signiert ist.
    pub signs: SignedTarget,
    /// URL der Signaturdatei (bei `gpg_detached` Pflicht), relativ oder absolut, Platzhalter wie bei `checksum`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Erlaubte Signaturschlüssel als 40-stellige Fingerprints (ohne Leerzeichen).
    /// Nur Signaturen von genau diesen Schlüsseln werden akzeptiert.
    pub fingerprints: Vec<String>,
    /// HTTPS-URL, von der die öffentlichen Schlüssel geladen werden können.
    /// Die geladenen Schlüssel werden trotzdem gegen `fingerprints` geprüft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_url: Option<String>,
    /// Alte v3-Signaturpakete akzeptieren (Standard: nein). Nur setzen, wenn der Hersteller
    /// nichts anderes liefert (openSUSE). Erzeugt eine Warnung bei der Katalogprüfung.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_v3: bool,
}

/// Art der GPG-Signatur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignatureKind {
    /// Separate Signaturdatei (`.gpg`, `.sig`, `.asc`, `.sign`).
    GpgDetached,
    /// Die Prüfsummendatei ist selbst signiert (`-----BEGIN PGP SIGNED MESSAGE-----`).
    GpgClearsigned,
}

/// Was von der Signatur abgedeckt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SignedTarget {
    /// Die Prüfsummendatei (üblich: Ubuntu, Debian, Mint, Fedora).
    ChecksumFile,
    /// Das Image direkt (Arch, Proxmox).
    Image,
}
