# Architektur & Entscheidungen

Diese Datei hält fest, **warum** BootForge so gebaut ist. Neue Entscheidungen werden unten ergänzt.

## Überblick

```text
┌──────────────── GUI (Svelte, läuft im WebView) ────────────────┐
│  Distro wählen → Stick wählen → Forge! → Fortschritt            │
└───────────────┬─────────────────────────────────▲──────────────┘
                │ invoke("get_catalog")            │ Events (Fortschritt, ab Phase 3)
┌───────────────▼─────────────────────────────────┴──────────────┐
│  src-tauri  –  Tauri-Commands, übersetzt Fehler für die GUI     │
└───────────────┬────────────────────────────────────────────────┘
                │ normale Rust-Funktionsaufrufe
┌───────────────▼────────────────────────────────────────────────┐
│  bootforge-core – Katalog, Versionsermittlung, Download,        │
│  Prüfsummen/GPG, USB-Erkennung, Flashen (keine GUI-Abhängigkeit)│
└────────────────────────────────────────────────────────────────┘
           (Linux, ab Phase 5: kleiner Flash-Helper via pkexec)
```

## Entscheidungen

### E1 – Cargo-Workspace mit eigenem `bootforge-core`
**Warum:** Die gefährlichen Teile (Laufwerke erkennen, Rohdaten schreiben) sollen mit
`cargo test` prüfbar sein, ohne ein Fenster zu öffnen. Außerdem soll der spätere Linux-Helper,
der mit Root-Rechten läuft, *dieselben* Sicherheitsprüfungen nutzen, aber möglichst wenig Code
enthalten. Ein Root-Prozess mit kompletter GUI wäre eine unnötig große Angriffsfläche.

### E2 – Svelte 5 statt Vanilla-TS oder React
**Warum:** Die GUI hat viel Zustand (Auswahl, Fortschritt, Fehler), der sich gegenseitig beeinflusst.
Mit Vanilla-TS müsste man das DOM selbst nachführen, was schnell unübersichtlich wird. Svelte
übersetzt die Komponenten beim Build in schlankes JavaScript (~50 KB) ohne Laufzeit-Framework,
und die Syntax liegt sehr nah an HTML/CSS/TS. Kein SvelteKit, weil wir weder Routing noch
Server-Rendering brauchen.

### E3 – Das Frontend darf fast nichts
`capabilities/default.json` erlaubt nur `core:default`. Das Frontend hat **keinen** direkten
Zugriff auf Dateisystem, Shell oder HTTP. Alles läuft über eigene Rust-Commands, die genau eine
Sache tun. Selbst wenn über manipulierte Katalogtexte HTML/JS ins WebView gelangen sollte, könnte
es keine Laufwerke beschreiben. Zusätzlich gilt eine strenge Content-Security-Policy (CSP).

### E4 – Katalogformat in Rust definiert, Schema generiert
Das Format steht nur an einer Stelle (`model.rs`). `schemars` erzeugt daraus das JSON-Schema,
und ein Test schlägt fehl, wenn die eingecheckte Schema-Datei veraltet ist. So können Code und
Doku nicht auseinanderlaufen.

### E5 – Zweistufige Katalogprüfung
1. **Form** (`serde`, `deny_unknown_fields`): Felder und Typen stimmen.
2. **Inhalt** (`validate.rs`): nur HTTPS, gültige Regexe, eindeutige IDs, Fingerprints im richtigen
   Format, Prüfsumme vorhanden.

Ein Katalog mit Fehlern wird **komplett** verworfen, nicht teilweise benutzt. Dann greifen der Cache
bzw. der eingebaute Katalog.

### E6 – Versionsermittlung über Verzeichnislistings statt eigenem Server
Die meisten Distros veröffentlichen ihre ISOs in Verzeichnissen mit fester Namenslogik. Mit
Regex-Schritten im Katalog lässt sich das ohne Code-Änderung beschreiben. Neue Distros brauchen
also nur einen Katalogeintrag und keine neue App-Version.

Beim Erstellen des Beispielkatalogs (01.11.2026) gegen die echten Server geprüft. Dabei fiel auf:
- Fedora-Mirror (`download.fedoraproject.org`) zeigt bereits ein leeres `45/`. Deshalb nutzen wir
  `dl.fedoraproject.org` fürs Listing und probieren bei Sackgassen die nächste Version.
- Einige Server liefern Links als `./datei` → Links werden vor dem Regex-Vergleich normalisiert.
- `download.truenas.com` liefert eine flache Liste aller Pfade → ein einziger `match` über den
  vollen Pfad reicht, und Beta/RC-Versionen fallen durch die reine Zahlen-Regex heraus.
- openSUSE Leap: Unter `distribution/leap/` liegt noch das alte `42.3/`, das numerisch „größer“
  als `16.1` ist → die Regex lässt nur 16+ zu.
- Batocera ist eine komprimierte `.img.gz` (kein ISO) mit nur MD5 → `compression: gzip`, Warnung.

### E7 – GPG-Fingerprints im Katalog fest hinterlegt („Pinning“)
Die Fingerprints wurden aus den aktuellen Signaturen gelesen **und** gegen die Schlüsselquellen der
Hersteller abgeglichen (Ubuntu, Mint, Debian, Arch über `keyserver.ubuntu.com`; Fedora über
`fedoraproject.org/fedora.gpg`; openSUSE über das Repo-Key-File; Proxmox über das
Archive-Keyring). Vor dem Release (Phase 3) noch einmal gegen die Webseiten der Hersteller prüfen.

### E8 – Fehler als Code + Detail
Fehler gehen als `{ code, detail }` an die GUI. Die GUI übersetzt den `code`, und `detail` ist
aufklappbar für Techniker. So bleibt die Mehrsprachigkeit im Frontend, und das Backend muss keine
Sprache kennen.

### E10 – Katalog in eigenem Repo
Der Katalog liegt in [`xHellPhoenix/bootforge-catalog`](https://github.com/xHellPhoenix/bootforge-catalog),
die App in `xHellPhoenix/bootforge`. **Warum:** Ein neuer Distro-Eintrag oder eine korrigierte Regex
soll sofort bei allen Nutzern ankommen, ohne neue App-Version. Und wer nur den Katalog pflegt, braucht
keinen Zugriff auf den App-Code. Die Kopie in `catalog/` wird beim Bauen als Fallback eingebettet.

### E9 – Eigene kleine i18n statt Bibliothek
`src/lib/i18n.svelte.ts` umfasst ~30 Zeilen: flache JSON-Dateien pro Sprache, Platzhalter `{name}`,
Fallback auf Deutsch. Für zwei Sprachen reicht das. Wird es mehr (Plural-Regeln, Datumsformate),
kann man auf eine Bibliothek wie `svelte-i18n` wechseln.

## Offene Punkte für spätere Phasen
- TypeScript-Typen werden noch von Hand gepflegt (`src/lib/types.ts`). Später eventuell mit
  `specta`/`ts-rs` generieren.
- Windows: `requireAdministrator`-Manifest in `build.rs` (Phase 5).
- Linux: polkit-Policy + Helper-Binary (Phase 5).
