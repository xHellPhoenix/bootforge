fn main() {
    // Windows: Release-Builds verlangen Administratorrechte (Manifest).
    // Debug-Builds nicht – sonst müsste man schon zum Entwickeln jedes Mal ein Admin-Terminal
    // öffnen (`cargo run` bricht sonst mit „Vorgang erfordert Erhöhung“ ab). Ohne Admin-Rechte
    // meldet die App beim Flashen einen verständlichen Fehler.
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let mut attrs = tauri_build::Attributes::new();
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        attrs = attrs.windows_attributes(
            tauri_build::WindowsAttributes::new()
                .app_manifest(include_str!("windows-app-manifest.xml")),
        );
    }
    tauri_build::try_build(attrs).expect("tauri-build fehlgeschlagen");
}
