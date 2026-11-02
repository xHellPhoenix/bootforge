use sealflash_core::catalog::{self, Severity};

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

/// Die Ausnahme „ohne Prüfsumme“ darf nicht durch die Hintertür in andere Einträge gelangen.
#[test]
fn origin_only_is_tightly_restricted() {
    use serde_json::{Value, json};
    let base: Value = serde_json::from_str(catalog::BUNDLED_CATALOG_JSON).unwrap();
    // Variante per ID finden und verändern.
    let edit = |id: &str, f: &dyn Fn(&mut Value)| {
        let mut c = base.clone();
        for d in c["distros"].as_array_mut().unwrap() {
            for v in d["variants"].as_array_mut().unwrap() {
                if v["id"] == id {
                    f(v);
                }
            }
        }
        match catalog::parse(&c.to_string()) {
            Err(catalog::CatalogError::Invalid(i)) => i
                .into_iter()
                .map(|i| i.message)
                .collect::<Vec<_>>()
                .join(" | "),
            other => panic!("erwartet Fehler, war {:?}", other.map(|_| ())),
        }
    };

    // Platzhalter im Hostnamen
    let e = edit("windows-server-2025-de", &|v| {
        v["checksum"]["allowed_hosts"] = json!(["*.microsoft.com"])
    });
    assert!(e.contains("kein gültiger Hostname"), "{e}");
    // Schema statt Hostname
    let e = edit("windows-server-2025-de", &|v| {
        v["checksum"]["allowed_hosts"] = json!(["https://download.microsoft.com"])
    });
    assert!(e.contains("kein gültiger Hostname"), "{e}");
    // leere Liste
    let e = edit("windows-server-2025-de", &|v| {
        v["checksum"]["allowed_hosts"] = json!([])
    });
    assert!(e.contains("mindestens ein erlaubter Server"), "{e}");
    // https_origin bei einer normalen Quelle (Windows 11: manual)
    let e = edit(
        "windows-11",
        &|v| {
            v["checksum"] = json!({"type": "https_origin", "allowed_hosts": ["www.microsoft.com"]})
        },
    );
    assert!(e.contains("nur bei `direct`"), "{e}");
    // direct mit einer echten Prüfsumme statt https_origin
    let e =
        edit(
            "windows-server-2025-de",
            &|v| {
                v["checksum"] = json!({"type": "file", "url": "SHA256SUMS", "algorithm": "sha256"})
            },
        );
    assert!(e.contains("`direct` braucht"), "{e}");
}
