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

### E11 – Katalog laden: Online → Cache → eingebaut
Die App startet **immer**, auch offline. Der Online-Katalog wird per ETag abgefragt: Hat er sich
nicht geändert, antwortet GitHub mit „304 Not Modified“ ohne Inhalt. Ein ungültiger Online-Katalog
überschreibt **nie** den guten Cache. Der Cache wird beim Lesen erneut geprüft, denn er könnte
beschädigt sein. Geschrieben wird atomar (temporäre Datei + Umbenennen), damit ein Absturz keine
halbe Datei hinterlässt. Die GUI zeigt an, woher der Katalog stammt, und warum, falls nicht online.

### E12 – `Fetcher`-Trait statt direktem `reqwest`
Der Resolver kennt nur das Trait `Fetcher` (`get`, `content_length`). In der App steckt dahinter
`reqwest`, in den Tests ein `FakeFetcher` mit vorbereiteten Antworten. So prüfen die Tests echte
Server-Eigenheiten (leeres Fedora-`45/`, Leaps `42.3/`, das flache TrueNAS-Listing) ohne Internet,
schnell und reproduzierbar. Dieses Muster heißt *Dependency Injection*.

### E13 – Sicherheit im HTTP-Client
- `https_only(true)`: Auch eine Weiterleitung von HTTPS auf HTTP wird abgelehnt.
- URLs aus Antworten (JSON-APIs) werden zusätzlich auf HTTPS geprüft.
- Textantworten sind auf 32 MB begrenzt (TrueNAS-Listing: ~12 MB) und werden beim Lesen mitgezählt,
  weil `Content-Length` fehlen oder falsch sein kann.
- Prüfsummen aus APIs müssen exakt zum Algorithmus passen (z. B. 64 Hex-Zeichen bei SHA-256).
- Pfade aus Listings mit `..` werden verworfen.
- Ein einziger automatischer Wiederholungsversuch bei Netzwerkfehlern, nicht bei HTTP-Fehlern.
  (Beim Testen gab es einen kurzen Aussetzer bei `mirrors.kernel.org`.)

### E14 – Begrenztes Backtracking
Ist die höchste Version eine Sackgasse (leeres Verzeichnis, 404), probiert der Resolver die
nächste, aber höchstens 3 pro Schritt. Ein kaputter Katalogeintrag kann so keinen Server mit
Anfragen fluten.

### E15 – Rust-Rekursion mit `async` braucht `Box::pin`
Das Durchlaufen der Listing-Schritte ist rekursiv. Ein `async fn`, das sich selbst aufruft, hätte
ein Future, das sich selbst enthält, also unendliche Größe. `Box::pin` legt das innere Future auf
den Heap; dann ist nur ein Zeiger im äußeren Future gespeichert.

### E16 – GPG mit Sequoia und nativem Krypto-Backend
GPG-Prüfung in der App selbst, statt das `gpg`-Programm aufzurufen: Unter Windows ist `gpg` meist
nicht installiert, und die Ausgabe von `gpg` zu parsen ist fehleranfällig. Sequoia ist eine
OpenPGP-Implementierung in Rust. Als Krypto-Backend nehmen wir das jeweils native:
**CNG** unter Windows (in Windows eingebaut), **Nettle** unter Linux (Systembibliothek, über GnuTLS
praktisch überall vorhanden). Das reine Rust-Backend stuft Sequoia selbst als experimentell ein.

### E17 – Vertrauensmodell: nur gepinnte Schlüssel
Der Schlüsselbund wird bei jeder Prüfung frisch über HTTPS geladen, aber **nur Zertifikate mit
einem Fingerprint aus dem Katalog** werden übernommen. Eine Signatur zählt nur, wenn sie
kryptografisch stimmt und von genau so einem Schlüssel stammt. Ein Angreifer, der den Schlüsselbund
oder die Signatur austauscht, scheitert also an den gepinnten Fingerprints.

Bei Clearsign-Dateien (Fedora) wird **nur der Text verwendet, den Sequoia als signiert bestätigt**,
nie der Rohtext. Sonst könnte jemand Zeilen außerhalb des signierten Bereichs einschmuggeln.

### E18 – Ausnahme: v3-Signaturen (openSUSE)
openSUSE signiert noch im alten v3-Format, das Sequoia seit 2021 ablehnt. Die Kryptografie
(RSA-4096 + SHA-512) ist stark, veraltet ist nur das Paketformat (keine geschützten Metadaten,
64-Bit-Schlüssel-ID). Weil wir ohnehin nur den gepinnten Schlüssel zulassen, ist das vertretbar.
Die Ausnahme ist **pro Variante** im Katalog anzuschalten (`allow_v3`) und erzeugt eine Warnung.
Die Alternative wäre, bei openSUSE ganz auf GPG zu verzichten, und das wäre schlechter.

### E19 – Reihenfolge: erst Signatur, dann Download
Prüfsummendatei und Signatur sind ein paar KB groß, das Image mehrere GB. Deshalb zuerst die
Signatur prüfen: Ist sie falsch, wird **kein einziges Byte** des Images geladen (ist per Test
abgesichert). Erst danach wird heruntergeladen. Der Hash läuft während des Downloads mit, sodass
die fertige Datei nicht noch einmal gelesen werden muss. Bei falschem Hash wird die Datei sofort
gelöscht.

### E20 – Fortsetzen mit `Range` + `If-Range`
Abgebrochene Downloads bleiben als `.part` liegen, daneben steht in `.part.json` die URL plus
ETag/Last-Modified. Beim nächsten Versuch wird der vorhandene Teil eingelesen (für den Hash) und
ab dort weitergeladen. `If-Range` stellt sicher, dass der Server nur dann ab der Mitte liefert,
wenn sich die Datei nicht geändert hat. Sonst schickt er sie komplett neu, und wir beginnen von vorn.

### E21 – Binärdaten nie als Text behandeln
Signaturen (`.gpg`, `.sig`) und manche Schlüsselbünde sind binär. Der erste Entwurf hat sie über
`get_text` geladen, was ungültiges UTF-8 ersetzt und die Signatur zerstört. Deshalb gibt es
`get_bytes` für alles, was geprüft wird; `get_text` ist nur für HTML-Listings und JSON da.

### E22 – Dateinamen vom Server misstrauen
Der Dateiname kommt vom Server. Bevor er als Pfad verwendet wird, wird geprüft: kein `/` oder `\`,
kein `..`, kein versteckter Name, keine unter Windows verbotenen Zeichen. Sonst könnte ein
manipulierter Server z. B. `../../.bashrc` überschreiben (Path Traversal).

## Offene Punkte für spätere Phasen
- **Kompatibilität des Online-Katalogs:** Wegen `deny_unknown_fields` lehnt eine ältere App einen
  Katalog mit neuen Feldern ab (z. B. `allow_v3`) und fällt auf ihren Cache zurück. Neue Felder darf
  der Online-Katalog deshalb erst bekommen, wenn die passende App-Version veröffentlicht ist.
  Langfristig: `schema_version` erhöhen und pro Version eine eigene Datei anbieten.
- Speicherplatz vor dem Download prüfen; Download-Ordner wählbar; alte ISOs aufräumen (Phase 7).
- GitHub Action im Katalog-Repo, die `resolve_all` regelmäßig laufen lässt und bei kaputten
  Einträgen ein Issue öffnet (Phase 7).
- TypeScript-Typen werden noch von Hand gepflegt (`src/lib/types.ts`). Später eventuell mit
  `specta`/`ts-rs` generieren.
- Windows: `requireAdministrator`-Manifest in `build.rs` (Phase 5).
- Linux: polkit-Policy + Helper-Binary (Phase 5).
