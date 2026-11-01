fn main() {
    // Erzeugt u. a. die Berechtigungs-Schemas für `capabilities/`.
    // In Phase 5 kommt hier das Windows-Manifest mit `requireAdministrator` dazu.
    tauri_build::build()
}
