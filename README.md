# BootForge

USB-Boot-Sticks erstellen, **ohne selbst ISOs suchen zu müssen**: Distribution wählen, Stick wählen, „Forge!“.
BootForge lädt die jeweils neueste Version direkt von der offiziellen Quelle, prüft sie (SHA-256 und GPG)
und schreibt sie auf den Stick.

[![CI](https://github.com/xHellPhoenix/bootforge/actions/workflows/ci.yml/badge.svg)](https://github.com/xHellPhoenix/bootforge/actions/workflows/ci.yml)

> **Status:** Phase 4 von 7. Neueste Version ermitteln, herunterladen und prüfen (SHA-256/512 und
> GPG-Signatur des Herstellers) funktionieren, USB-Sticks werden erkannt. Das Schreiben auf den
> Stick fehlt noch.

| Version wählen | Herunterladen | Geprüft | USB-Stick wählen |
|---|---|---|---|
| ![](docs/screenshots/phase2-version.png) | ![](docs/screenshots/phase3-progress.png) | ![](docs/screenshots/phase3-done.png) | ![](docs/screenshots/phase4-hidden.png) |

## Aufbau

| Pfad | Inhalt |
|---|---|
| `crates/bootforge-core/` | Die eigentliche Logik in Rust, ohne GUI und testbar |
| `src-tauri/` | Tauri-v2-App: dünne Schicht zwischen GUI und Core |
| `src/` | GUI (Svelte 5 + TypeScript) |
| `catalog/catalog.json` | Eingebauter Offline-Fallback des Katalogs. Der aktuelle Katalog liegt in [bootforge-catalog](https://github.com/xHellPhoenix/bootforge-catalog) |
| `catalog/catalog.schema.json` | JSON-Schema, **generiert** aus dem Rust-Code |
| `docs/` | [Architektur & Entscheidungen](docs/ARCHITEKTUR.md), [Katalogformat](docs/KATALOG.md) |

## Entwickeln

Voraussetzungen: Rust (stable), Node.js ≥ 20, unter Linux zusätzlich
`libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libssl-dev` ([Tauri-Voraussetzungen](https://v2.tauri.app/start/prerequisites/))
sowie für die GPG-Prüfung `nettle-dev clang` (Debian/Ubuntu-Paketnamen). Unter Windows ist nichts
weiter nötig, dort wird die in Windows eingebaute Kryptografie (CNG) verwendet.

```bash
npm install
npm run tauri dev        # App mit Hot-Reload starten
cargo test --workspace   # Rust-Tests (u. a. Katalog-Validierung)
npm run check            # TypeScript/Svelte prüfen
```

Alle Katalogeinträge gegen die echten Server prüfen (zeigt die aktuell neueste Version jeder Variante):

```bash
cargo run -p bootforge-core --example resolve_all                  # eingebauter Katalog
cargo run -p bootforge-core --example resolve_all ../bootforge-catalog/catalog.json
```

Eine Variante ohne GUI herunterladen und prüfen (Strg+C bricht ab, erneuter Aufruf setzt fort):

```bash
cargo run --release -p bootforge-core --example acquire_one -- debian-netinst /tmp/isos
```

Laufwerke mit Urteil anzeigen (schreibt nichts). Zum Testen mit echten Sticks siehe
[docs/TEST-USB.md](docs/TEST-USB.md):

```bash
cargo run -p bootforge-core --example list_drives
```

Einen anderen Katalog in der App testen (nur HTTPS):

```bash
BOOTFORGE_CATALOG_URL=https://example.org/mein-katalog.json npm run tauri dev
```

Nach Änderungen am Katalog-Datenmodell (`model.rs`) das Schema neu erzeugen:

```bash
cargo run -p bootforge-core --example gen_schema > catalog/catalog.schema.json
```

## Roadmap

1. ✅ Projektstruktur, Tauri-Grundgerüst, Katalog-Schema + Beispielkatalog
2. ✅ Katalog über HTTPS laden (mit Cache) + „neueste Version“ ermitteln
3. ✅ Download + Prüfsumme + GPG
4. ✅ USB-Erkennung (Windows + Linux)
5. Raw-Flashen + Verifizierung
6. Windows-ISOs + Windows-Flash-Modus
7. GUI-Feinschliff, Builds, Doku
