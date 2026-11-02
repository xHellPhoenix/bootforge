use bootforge_core::acquire::AcquireError;
use bootforge_core::download::DownloadError;
use bootforge_core::flash::FlashError;
use bootforge_core::http::FetchError;
use bootforge_core::resolve::ResolveError;
use serde::Serialize;

/// Fehler, wie sie an die GUI gehen.
///
/// Statt nur eines Textes schicken wir einen `code` mit. Die GUI übersetzt den Code
/// (Deutsch/Englisch) und zeigt `detail` aufklappbar für Techniker an.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum AppError {
    #[error("Katalog noch nicht geladen")]
    CatalogNotLoaded,
    #[error("unbekannte Variante: {0}")]
    UnknownVariant(String),
    #[error("Netzwerkfehler: {0}")]
    Network(String),
    #[error("Server-Fehler: {0}")]
    Server(String),
    #[error("Version nicht gefunden: {0}")]
    NoMatch(String),
    #[error("Noch nicht unterstützt: {0}")]
    NotSupportedYet(String),
    #[error("Unsichere Quelle: {0}")]
    Insecure(String),
    #[error("Es läuft bereits ein Download")]
    Busy,
    #[error("abgebrochen")]
    Cancelled,
    #[error("Prüfsumme stimmt nicht: {0}")]
    ChecksumMismatch(String),
    #[error("Signatur ungültig: {0}")]
    SignatureInvalid(String),
    #[error("Prüfsumme fehlt: {0}")]
    ChecksumMissing(String),
    #[error("Dateifehler: {0}")]
    Disk(String),
    #[error("Laufwerke konnten nicht gelesen werden: {0}")]
    DeviceScan(String),
    // --- Eigene Images ---
    #[error("Diese Variante lädt man selbst herunter")]
    ManualSource,
    #[error("Prüfsumme ungültig (erwartet: SHA-256, SHA-512 oder MD5 in Hex)")]
    InvalidHash,
    #[error("Das ist kein Windows-Installationsmedium")]
    NotWindowsMedia,
    #[error("Prüfsumme steht nicht in der Liste des Herstellers: {0}")]
    NotOnVendorList(String),
    #[error("Datei wurde noch nicht geprüft")]
    NotVerified,
    #[error("Programm wimlib-imagex fehlt")]
    WimlibMissing,
    // --- Flashen ---
    #[error("Bestätigung stimmt nicht mit dem Laufwerksnamen überein")]
    ConfirmMismatch,
    #[error("Laufwerk hat sich geändert: {0}")]
    DriveChanged(String),
    #[error("Image passt nicht zur Prüfsumme: {0}")]
    ImageMismatch(String),
    #[error("Image wurde während des Schreibens verändert")]
    ImageChanged,
    #[error("Stick zu klein: {0}")]
    TooSmall(String),
    #[error("Zurücklesen fehlgeschlagen: {0}")]
    ReadbackMismatch(String),
    #[error("Laufwerk wird verwendet: {0}")]
    DriveBusy(String),
    #[error("Administratorrechte nötig")]
    NotElevated,
    /// Nur Linux: pkexec-Dialog abgebrochen.
    #[cfg(target_os = "linux")]
    #[error("Anmeldung abgebrochen oder abgelehnt")]
    AuthCancelled,
    /// Nur Linux: Root-Helper nicht gefunden.
    #[cfg(target_os = "linux")]
    #[error("Hilfsprogramm nicht gefunden: {0}")]
    HelperMissing(String),
    /// Abgebrochen, nachdem schon geschrieben wurde: der Stick ist nicht mehr bootbar.
    #[error("abgebrochen während des Schreibens")]
    FlashCancelled,
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// `wrote`: hatte das Schreiben schon begonnen? (wichtig für die Abbruch-Meldung)
    pub fn from_flash(e: FlashError, wrote: bool) -> Self {
        let detail = e.to_string();
        match e {
            FlashError::Drive(d) => AppError::DriveChanged(d),
            FlashError::ImageMismatch { .. } => AppError::ImageMismatch(detail),
            FlashError::ImageChanged => AppError::ImageChanged,
            FlashError::TooSmall { .. } => AppError::TooSmall(detail),
            FlashError::ReadbackMismatch { .. } => AppError::ReadbackMismatch(detail),
            FlashError::Busy(b) => AppError::DriveBusy(b),
            FlashError::NotElevated => AppError::NotElevated,
            FlashError::Unsupported(u) if u.contains("wimlib-imagex") => AppError::WimlibMissing,
            FlashError::Unsupported(u) => AppError::NotSupportedYet(u),
            FlashError::Cancelled if wrote => AppError::FlashCancelled,
            FlashError::Cancelled => AppError::Cancelled,
            FlashError::Io(_) => AppError::Disk(detail),
        }
    }
}

impl From<AcquireError> for AppError {
    fn from(e: AcquireError) -> Self {
        let detail = e.to_string();
        match e {
            AcquireError::Cancelled => AppError::Cancelled,
            AcquireError::ChecksumMismatch { .. } => AppError::ChecksumMismatch(detail),
            AcquireError::Gpg(_) | AcquireError::MissingKeyring => {
                AppError::SignatureInvalid(detail)
            }
            AcquireError::ChecksumNotListed { .. } => AppError::ChecksumMissing(detail),
            AcquireError::BadFilename(_) => AppError::Insecure(detail),
            AcquireError::Fetch(f) | AcquireError::Download(DownloadError::Fetch(f)) => {
                ResolveError::Fetch(f).into()
            }
            AcquireError::Download(DownloadError::Io(_)) => AppError::Disk(detail),
            AcquireError::Download(_) => AppError::Network(detail),
        }
    }
}

impl From<ResolveError> for AppError {
    fn from(e: ResolveError) -> Self {
        let detail = e.to_string();
        match e {
            ResolveError::Fetch(FetchError::Network { .. }) => AppError::Network(detail),
            ResolveError::Fetch(FetchError::NotHttps(_)) => AppError::Insecure(detail),
            ResolveError::Fetch(_) => AppError::Server(detail),
            ResolveError::NoMatch { .. }
            | ResolveError::MissingField { .. }
            | ResolveError::Json { .. } => AppError::NoMatch(detail),
            ResolveError::NotSupportedYet(what) => AppError::NotSupportedYet(what.into()),
            ResolveError::Manual => AppError::ManualSource,
            ResolveError::BadChecksumValue { .. }
            | ResolveError::BadPattern(_)
            | ResolveError::BadUrl(_) => AppError::Internal(detail),
        }
    }
}
