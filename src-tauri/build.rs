/// Windows-Manifest für Release-Builds (`windows-app-manifest.xml`): BootForge startet immer mit
/// Administratorrechten (UAC-Abfrage), weil das Schreiben auf `\\.\PhysicalDriveN` sie erfordert.
/// Die Common-Controls-Abhängigkeit entspricht Tauris Standard-Manifest (moderne Dialoge).
///
/// **Das Manifest muss reines ASCII sein und darf keine `<?xml …?>`-Zeile haben.** Tauri bettet es
/// als Text in eine Ressourcen-Datei ein. Umlaute werden dabei in falsche Bytes umgewandelt, und vor
/// der XML-Zeile landet ein Leerzeichen. Windows lehnt die Datei dann ab („Side-by-Side-Konfiguration
/// ist ungültig“), und die App startet gar nicht (passiert in v0.1.0). Deshalb steht diese
/// Erklärung hier und nicht als Kommentar im Manifest, und die Prüfung unten bricht den Build ab.
const MANIFEST: &str = include_str!("windows-app-manifest.xml");

fn main() {
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    assert!(
        MANIFEST.is_ascii(),
        "windows-app-manifest.xml darf nur ASCII-Zeichen enthalten (keine Umlaute)"
    );
    assert!(
        MANIFEST.trim_start().starts_with("<assembly"),
        "windows-app-manifest.xml muss direkt mit <assembly beginnen (keine <?xml-Zeile, keine Kommentare)"
    );

    // Release-Builds verlangen Administratorrechte. Debug-Builds nicht, sonst müsste man schon zum
    // Entwickeln jedes Mal ein Admin-Terminal öffnen (`cargo run` bricht sonst mit „Vorgang
    // erfordert Erhöhung“ ab). Ohne Admin-Rechte meldet die App beim Flashen einen verständlichen
    // Fehler.
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        attrs =
            attrs.windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(MANIFEST));
    }
    tauri_build::try_build(attrs).expect("tauri-build fehlgeschlagen");
}
