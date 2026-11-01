use bootforge_core::catalog::{self, Severity};

/// Der mitgelieferte Katalog muss fehlerfrei sein.
#[test]
fn bundled_catalog_is_valid() {
    let (cat, warnings) = catalog::bundled().unwrap_or_else(|e| panic!("{e}"));
    assert!(!cat.distros.is_empty());
    assert!(warnings.iter().all(|w| w.severity == Severity::Warning));
    for w in &warnings {
        eprintln!("{w}");
    }
}

/// Die eingecheckte Schema-Datei muss exakt zum Rust-Code passen.
/// Wenn dieser Test fehlschlägt: Schema neu erzeugen (siehe examples/gen_schema.rs).
#[test]
fn schema_file_is_up_to_date() {
    let committed = include_str!("../../../catalog/catalog.schema.json");
    assert_eq!(
        committed,
        catalog::json_schema(),
        "catalog.schema.json ist veraltet – bitte neu erzeugen"
    );
}

#[test]
fn rejects_http_and_bad_regex() {
    let json = catalog::BUNDLED_CATALOG_JSON
        .replacen(
            "https://releases.ubuntu.com/",
            "http://releases.ubuntu.com/",
            1,
        )
        .replacen(r#""match": "^archlinux-"#, r#""match": "^(archlinux-"#, 1);
    let err = catalog::parse(&json).unwrap_err();
    let catalog::CatalogError::Invalid(issues) = err else {
        panic!("falscher Fehlertyp")
    };
    assert!(
        issues.iter().any(|i| i.message.contains("nur HTTPS")),
        "{issues:?}"
    );
    assert!(
        issues.iter().any(|i| i.message.contains("ungültige Regex")),
        "{issues:?}"
    );
}

#[test]
fn rejects_unknown_fields() {
    let json = catalog::BUNDLED_CATALOG_JSON.replacen(
        r#""schema_version""#,
        r#""tippfehler": 1, "schema_version""#,
        1,
    );
    assert!(matches!(
        catalog::parse(&json),
        Err(catalog::CatalogError::Parse(_))
    ));
}

#[test]
fn localized_text_falls_back_to_german() {
    let (cat, _) = catalog::bundled().unwrap();
    let d = &cat.distros[0];
    assert_eq!(d.description.get("xx"), d.description.get("de"));
}
