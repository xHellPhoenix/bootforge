//! GPG-Signaturen prüfen (mit Sequoia-PGP).
//!
//! Das Vertrauensmodell ist bewusst einfach und streng:
//!
//! 1. Der Katalog nennt die **erlaubten Fingerprints** (gepinnt).
//! 2. Aus dem heruntergeladenen Keyring werden **nur** Zertifikate mit diesen Fingerprints übernommen.
//!    Ein manipulierter Keyring kann also keinen fremden Schlüssel einschleusen.
//! 3. Eine Signatur zählt nur, wenn sie kryptografisch stimmt **und** von einem dieser Schlüssel
//!    stammt (Hauptschlüssel oder dessen Unterschlüssel).
//!
//! Es gibt kein „Web of Trust“ und keinen lokalen GPG-Schlüsselbund – die App ist unabhängig davon,
//! was auf dem Rechner installiert ist.

use std::io::Read;

use sequoia_openpgp as openpgp;

use openpgp::Cert;
use openpgp::KeyHandle;
use openpgp::cert::CertParser;
use openpgp::packet::Tag;
use openpgp::parse::Parse;
use openpgp::parse::stream::{
    DetachedVerifierBuilder, MessageLayer, MessageStructure, VerificationHelper, VerifierBuilder,
};
use openpgp::policy::StandardPolicy;
use serde::Serialize;

/// Wer hat signiert? (Für die Anzeige in der GUI.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signer {
    /// Fingerprint des Hauptschlüssels.
    pub fingerprint: String,
    /// Erste User-ID, z. B. „Ubuntu CD Image Automatic Signing Key (2012) <cdimage@ubuntu.com>“.
    pub user_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GpgError {
    #[error("der Schlüsselbund enthält keinen der erlaubten Schlüssel ({0})")]
    NoPinnedKey(String),
    #[error("Schlüsselbund unlesbar: {0}")]
    BadKeyring(String),
    #[error("Signatur ungültig: {0}")]
    BadSignature(String),
}

/// Normalisiert Fingerprints für den Vergleich (Großbuchstaben, ohne Leerzeichen).
fn norm(fp: &str) -> String {
    fp.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Liest einen Schlüsselbund (ASCII-Armor oder binär) und behält nur gepinnte Zertifikate.
pub fn pinned_certs(keyring: &[u8], fingerprints: &[String]) -> Result<Vec<Cert>, GpgError> {
    let pinned: Vec<String> = fingerprints.iter().map(|f| norm(f)).collect();
    let parser =
        CertParser::from_bytes(keyring).map_err(|e| GpgError::BadKeyring(e.to_string()))?;
    let mut out = Vec::new();
    for cert in parser {
        // Einzelne kaputte Zertifikate in großen Schlüsselbünden (Fedora) überspringen.
        let Ok(cert) = cert else { continue };
        let matches = cert
            .keys()
            .any(|k| pinned.contains(&k.key().fingerprint().to_hex()));
        if matches {
            out.push(cert);
        }
    }
    if out.is_empty() {
        return Err(GpgError::NoPinnedKey(pinned.join(", ")));
    }
    Ok(out)
}

/// Verbindet Sequoias Prüf-Ablauf mit unseren gepinnten Zertifikaten.
struct Helper {
    certs: Vec<Cert>,
    pinned: Vec<String>,
    signer: Option<Signer>,
}

impl VerificationHelper for Helper {
    fn get_certs(&mut self, _ids: &[KeyHandle]) -> openpgp::Result<Vec<Cert>> {
        // Nur die gepinnten Zertifikate anbieten – alles andere ist für uns „unbekannt“.
        Ok(self.certs.clone())
    }

    fn check(&mut self, structure: MessageStructure) -> openpgp::Result<()> {
        let mut problems = Vec::new();
        for layer in structure.into_iter() {
            match layer {
                MessageLayer::SignatureGroup { results } => {
                    for result in results {
                        match result {
                            Ok(good) => {
                                let cert = good.ka.cert();
                                let primary = cert.fingerprint().to_hex();
                                let signing = good.ka.key().fingerprint().to_hex();
                                if self.pinned.contains(&primary) || self.pinned.contains(&signing)
                                {
                                    self.signer = Some(Signer {
                                        fingerprint: primary,
                                        user_id: cert
                                            .userids()
                                            .next()
                                            .map(|u| u.userid().to_string()),
                                    });
                                    return Ok(());
                                }
                                problems.push(format!("Schlüssel {signing} ist nicht erlaubt"));
                            }
                            Err(e) => problems.push(e.to_string()),
                        }
                    }
                }
                // Verschlüsselte oder komprimierte Ebenen gibt es bei Prüfsummendateien nicht.
                _ => problems.push("unerwarteter Nachrichtenaufbau".into()),
            }
        }
        if problems.is_empty() {
            problems.push("keine Signatur gefunden".into());
        }
        Err(openpgp::Error::InvalidOperation(problems.join("; ")).into())
    }
}

fn helper(certs: &[Cert], fingerprints: &[String]) -> Helper {
    Helper {
        certs: certs.to_vec(),
        pinned: fingerprints.iter().map(|f| norm(f)).collect(),
        signer: None,
    }
}

/// Sequoias Standard-Richtlinie (lehnt u. a. SHA-1 und v3-Signaturen ab).
/// `allow_v3` lockert genau eine Regel, für Hersteller wie openSUSE.
fn policy(allow_v3: bool) -> StandardPolicy<'static> {
    let mut p = StandardPolicy::new();
    if allow_v3 {
        p.accept_packet_tag_version(Tag::Signature, 3);
    }
    p
}

fn bad(e: openpgp::anyhow::Error) -> GpgError {
    // Die ganze Ursachenkette ausgeben – die eigentliche Info steckt oft innen.
    GpgError::BadSignature(format!("{e:#}"))
}

/// Separate Signatur (`.gpg`, `.sig`, `.asc`, `.sign`) über Daten aus einem Reader prüfen.
/// Für große Dateien (Image) einen `File` übergeben – es wird gestreamt, nicht komplett geladen.
pub fn verify_detached<R: Read + Send + Sync>(
    data: R,
    signature: &[u8],
    certs: &[Cert],
    fingerprints: &[String],
    allow_v3: bool,
) -> Result<Signer, GpgError> {
    let policy = policy(allow_v3);
    let mut v = DetachedVerifierBuilder::from_bytes(signature)
        .map_err(bad)?
        .with_policy(&policy, None, helper(certs, fingerprints))
        .map_err(bad)?;
    v.verify_reader(data).map_err(bad)?;
    v.into_helper()
        .signer
        .ok_or_else(|| GpgError::BadSignature("kein Unterzeichner".into()))
}

/// Clearsign-Nachricht prüfen und **nur den signierten Text** zurückgeben.
pub fn verify_cleartext(
    message: &[u8],
    certs: &[Cert],
    fingerprints: &[String],
    allow_v3: bool,
) -> Result<(String, Signer), GpgError> {
    let policy = policy(allow_v3);
    let mut v = VerifierBuilder::from_bytes(message)
        .map_err(bad)?
        .with_policy(&policy, None, helper(certs, fingerprints))
        .map_err(bad)?;
    let mut text = Vec::new();
    // Wichtig: Der Verifier gibt Daten erst frei, wenn die Signatur geprüft ist; Lesefehler = ungültig.
    v.read_to_end(&mut text)
        .map_err(|e| GpgError::BadSignature(e.to_string()))?;
    if !v.message_processed() {
        return Err(GpgError::BadSignature("Nachricht unvollständig".into()));
    }
    let signer = v
        .into_helper()
        .signer
        .ok_or_else(|| GpgError::BadSignature("kein Unterzeichner".into()))?;
    Ok((String::from_utf8_lossy(&text).into_owned(), signer))
}
