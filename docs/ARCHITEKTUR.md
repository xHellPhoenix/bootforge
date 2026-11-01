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

### E23 – Laufwerke: Rohdaten und Urteil getrennt
Der Plattform-Code (`device/linux.rs`, `device/windows.rs`) sammelt nur **Rohdaten**. Ob ein
Laufwerk beschrieben werden darf, entscheidet allein `device::classify`: eine reine Funktion ohne
Systemzugriff, für Linux und Windows dieselbe, vollständig getestet. Prinzip **„Default deny“**:
Ein Laufwerk ist nur wählbar, wenn *jede* Regel erfüllt ist. Fehlt eine Information, gilt das
Laufwerk als verboten (unter Windows z. B. unbekanntes `IsBoot` → als System behandelt).

Die Regeln in der Reihenfolge ihrer Priorität:
1. **System:** trägt `/`, `/boot`, `/home` … oder Swap (Linux), bzw. `IsBoot`, `IsSystem` oder die
   Auslagerungsdatei (Windows). Gilt **auch für USB**, z. B. wenn Linux gerade vom Stick läuft.
2. **Nur USB** (Bus-Typ).
3. **Zweite, unabhängige Prüfung:** Linux: Gerätename `sdX`. Windows: Das Systemlaufwerk
   (`%SystemDrive%`, meist `C:`) liegt nicht darauf, unabhängig von dem, was Windows bei
   `IsBoot`/`IsSystem` meldet.
4. Nicht schreibgeschützt, nicht von LVM/RAID/LUKS belegt, Medium eingelegt.
5. Höchstens 2 TB, ohne Ausnahme.
6. **Externe Festplatten** (nicht als Wechselmedium gemeldet und über 128 GB) sind ausgeblendet,
   außer man setzt ausdrücklich das Häkchen. Auf solchen Platten liegen oft Backups. Viele moderne
   Sticks melden sich übrigens *nicht* als Wechselmedium, deshalb reicht dieses Merkmal allein
   nicht als Regel.

### E24 – Linux: `/sys` statt `lsblk`
`/sys/block` ist die stabile Kernel-Schnittstelle (die auch `lsblk` liest). So hängen wir nicht von
einem externen Programm und dessen Ausgabeformat ab. Eingebundene Partitionen werden über die
**Gerätenummer** (Major:Minor) mit `/proc/self/mountinfo` abgeglichen, nicht über Pfadnamen, die
auch `/dev/disk/by-uuid/…` heißen können. LVM/LUKS/RAID wird über `holders/` erkannt; trägt so ein
Gerät `/` oder `/home`, gilt das USB-Laufwerk als Systemlaufwerk.

### E25 – Windows: WMI (`MSFT_Disk`)
`MSFT_Disk` liefert `BusType`, `IsBoot` und `IsSystem` direkt; dazu kommen `MSFT_Partition`
(Laufwerksbuchstaben), `Win32_DiskDrive` (Wechselmedium) und `Win32_PageFileUsage`. Die Umrechnung
in Rohdaten (`windows_map.rs`) ist reiner Code und wird auch unter Linux getestet. Eine WMI-Verbindung
darf den Thread nicht wechseln (COM), deshalb läuft die Erkennung über `spawn_blocking`.

Stolperstein: `wmi` erlaubt `windows`/`windows-core` in einem Versionsbereich, und Cargo hat eine
inkompatible Mischung gewählt (0.61 + 0.62). Gelöst im `Cargo.lock` (beide auf dieselbe Version wie
Tauri); das CI baut mit `--locked`, damit das nicht unbemerkt verloren geht.

### E26 – Aktualisieren per Abfrage alle 2 s
Solange der Auswahldialog offen ist, wird die Liste alle 2 Sekunden neu eingelesen. Echte
Hotplug-Benachrichtigungen (udev bzw. `WM_DEVICECHANGE`) wären eleganter, sind aber pro System
aufwendig. Ein Scan dauert nur Millisekunden. Vor dem Schreiben (Phase 5) wird das Laufwerk
ohnehin noch einmal frisch geprüft.

### E27 – Entwickler-Schalter nur in Debug-Builds
`BOOTFORGE_DEV_SYSROOT` lässt die App einen nachgebauten `/sys`-Baum lesen, um die GUI ohne echten
Stick zu testen. Der Code dafür steht hinter `cfg(debug_assertions)` und ist in Release-Builds gar
nicht vorhanden, damit niemand einer echten Installation ein falsches Laufwerk unterschieben kann.

### E28 – CI unter Linux und Windows
GitHub Actions baut und testet jeden Push auf `ubuntu-24.04` und `windows-latest`. So wird auch
der Windows-Code (WMI, CNG) wirklich ausgeführt. `.gitattributes` erzwingt LF-Zeilenenden und
schützt die Signatur-Fixtures: Würde Git sie unter Windows nach CRLF umwandeln, wären die echten
Signaturen ungültig.

### E29 – Dreifache Prüfung beim Schreiben
1. **Vor dem Schreiben** wird die Image-Datei noch einmal gehasht. Stimmt der Hash nicht, wird der
   Stick gar nicht angefasst.
2. **Während des Schreibens** wird die Datei erneut gehasht. Wurde sie zwischen Prüfung und Schreiben
   ausgetauscht (TOCTOU: „time of check to time of use“), wird der Anfang des Sticks genullt, damit
   er nicht mit Mischdaten bootet.
3. **Danach wird zurückgelesen** und mit dem Geschriebenen verglichen. Das erkennt defekte Sticks und
   **gefälschte** Sticks, die mehr Speicher vorgeben, als sie haben (sie überschreiben dann den Anfang
   wieder). Der Test `fake_capacity_stick_detected` simuliert genau das.

Schreibzugriffe gehen am Cache vorbei (Linux `O_DIRECT`, Windows `FILE_FLAG_NO_BUFFERING`), sonst
käme das Zurücklesen aus dem RAM und würde nichts beweisen. Dafür müssen Puffer im Speicher auf
4096 Bytes ausgerichtet sein (`AlignedBuf`), und jeder Schreibzugriff muss ein Vielfaches davon
sein. Der letzte Block wird deshalb mit Nullen aufgefüllt.

### E30 – Laufwerk unmittelbar vor dem Schreiben neu prüfen
Zwischen Auswahl und „Forge!“ kann viel passieren. Unter Linux bekommt ein neu eingesteckter Stick
sogar oft denselben Namen `/dev/sdb`. `device::recheck` scannt deshalb neu, wendet alle Regeln erneut
an und vergleicht Größe, Name und Seriennummer. Diese Prüfung läuft **in dem Prozess, der schreibt**
(unter Linux also im Root-Helper), und verlässt sich nicht auf das, was die GUI schickt.

### E31 – Bestätigung durch Abtippen, im Backend erneut geprüft
Ein versehentlicher Klick darf keinen Stick löschen, deshalb muss man den Namen des Sticks abtippen.
Der Command `forge` prüft den eingetippten Namen **noch einmal selbst**. Ein Fehler in der GUI kann die
Bestätigung so nicht aushebeln. Nach einem Fehler gibt es bewusst kein „Nochmal“, nur „Zurück“:
Jeder Schreibversuch braucht eine neue Bestätigung.

### E32 – Linux: kleiner Root-Helper über pkexec
Die GUI läuft als normaler Nutzer. WebKitGTK mit Root-Rechten wäre eine riesige Angriffsfläche.
Geschrieben wird von `bootforge-flash-helper`: kein Netzwerk, keine GUI, eine Aufgabe. Er bekommt die
Anfrage als JSON über stdin und meldet Fortschritt als JSON-Zeilen. Er **misstraut** der Anfrage:
- Das Laufwerk wird neu erkannt und muss alle Regeln erfüllen.
- Das Image muss eine normale Datei sein (kein Symlink) und dem aufrufenden Nutzer gehören
  (`PKEXEC_UID`). Sonst könnte man ihn benutzen, um z. B. `/etc/shadow` auf einen Stick zu kopieren.
  Schon der Hash-Zwang verhindert das (man müsste den Hash der geheimen Datei kennen); die
  Eigentümerprüfung ist eine zweite Sperre.
- Schließt die App stdin (auch bei einem Absturz), bricht der Helper ab. Ein Root-Prozess soll nicht
  verwaist weiterschreiben.

Die polkit-Regel verlangt jedes Mal das Admin-Passwort (`auth_admin`, kein `_keep`). Vor dem
Öffnen werden alle Partitionen ausgehängt, dann wird das Gerät mit `O_EXCL` geöffnet: Benutzt noch
irgendetwas das Gerät, verweigert der Kernel das, eine letzte Absicherung.

### E33 – Windows: Admin-Manifest nur im Release-Build
Laut Anforderung braucht die App unter Windows Administratorrechte (Manifest `requireAdministrator`).
Das gilt nur für Release-Builds, sonst müsste man schon zum Entwickeln jedes Mal ein Admin-Terminal
öffnen. Ohne Adminrechte meldet die App beim Schreiben einen verständlichen Fehler. Vor dem Schreiben
werden alle Volumes des Sticks gesperrt (`FSCTL_LOCK_VOLUME`) und ausgehängt; die Sperren bleiben bis
zum Ende offen, sonst hängt Windows sie sofort wieder ein.

### E34 – Abhängigkeiten im Debug-Build optimieren
`[profile.dev.package."*"] opt-level = 2`: Ohne diese Einstellung hasht ein Debug-Build nur ~15 MB/s,
und ein Test mit einem echten Image dauert Minuten. Der eigene Code bleibt unoptimiert, damit er
schnell kompiliert und gut zu debuggen ist.

### E35 – `rust-version` bestimmt, welche Paketversionen Cargo wählt
Mit `resolver = "3"` wählt Cargo nur Paketversionen, die mit der angegebenen Mindest-Rust-Version
(`rust-version`) funktionieren. Bei `1.85` blieb Tauri deshalb auf 2.11, weil 2.12 Rust 1.90 braucht.
Die npm-Pakete waren aber schon 2.12, und `tauri dev` meldete einen Versions-Mismatch. Jetzt steht
dort `1.90`, und Rust- und npm-Seite von Tauri sind wieder auf derselben Minor-Version.

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
- Linux-Pakete: Helper nach `/usr/lib/bootforge/`, polkit-Regel nach `/usr/share/polkit-1/actions/` (Phase 7).
- Komprimierung `xz`/`zstd` beim Schreiben (bisher nur `gzip`, mehr braucht der Katalog noch nicht).
