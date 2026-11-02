//! Distro-Katalog: Datenmodell, Laden/Parsen und Validierung.
//!
//! - `model`: Datenmodell (= Katalogformat)
//! - `validate`: inhaltliche Prüfung
//! - `load`: Laden über HTTPS mit Cache und eingebautem Fallback

pub mod load;
pub mod model;
pub mod validate;

pub use model::*;
pub use validate::{Issue, Severity, validate};

/// Der Katalog, der beim Bauen in die App eingebettet wird. Er dient als letzter Fallback,
/// wenn weder das Internet noch ein Cache verfügbar ist.
pub const BUNDLED_CATALOG_JSON: &str = include_str!("../../../../catalog/catalog.json");

/// Fehler beim Einlesen eines Katalogs.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Katalog ist kein gültiges JSON bzw. passt nicht zum Format: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Katalog enthält {} Fehler, z. B.: {}", .0.len(), .0.first().map(ToString::to_string).unwrap_or_default())]
    Invalid(Vec<Issue>),
}

/// JSON parsen **und** validieren. Warnungen werden zurückgegeben, Fehler führen zur Ablehnung.
pub fn parse(json: &str) -> Result<(Catalog, Vec<Issue>), CatalogError> {
    let catalog: Catalog = serde_json::from_str(json)?;
    let issues = validate(&catalog);
    let (errors, warnings): (Vec<_>, Vec<_>) = issues
        .into_iter()
        .partition(|i| i.severity == Severity::Error);
    if errors.is_empty() {
        Ok((catalog, warnings))
    } else {
        Err(CatalogError::Invalid(errors))
    }
}

/// Den eingebetteten Katalog laden.
pub fn bundled() -> Result<(Catalog, Vec<Issue>), CatalogError> {
    parse(BUNDLED_CATALOG_JSON)
}

/// JSON-Schema des Katalogs als formatierter String (für `catalog/catalog.schema.json`).
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(Catalog);
    let mut s = serde_json::to_string_pretty(&schema).expect("Schema ist serialisierbar");
    s.push('\n');
    s
}
