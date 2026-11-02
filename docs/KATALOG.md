# Katalogformat

Der Katalog ist eine JSON-Datei (`catalog/catalog.json`). Das maßgebliche Format ist das
JSON-Schema `catalog/catalog.schema.json`; es wird aus `crates/bootforge-core/src/catalog/model.rs`
generiert. Editoren wie VS Code nutzen es automatisch (wegen `"$schema"` in der Datei) für
Autovervollständigung und Fehlermarkierungen.

## Aufbau

```text
Katalog
├── categories[]         Filter-Chips (desktop, server, homelab, gaming, windows)
└── distros[]            z. B. Ubuntu
    └── variants[]       z. B. Desktop (LTS), Server (LTS)
        ├── source       Wie wird die neueste Version gefunden?
        ├── checksum     Woher kommt die Prüfsumme? (Pflicht)
        └── signature    Optional: GPG-Signatur + erlaubte Schlüssel
```

## `source`: Neueste Version finden

### `directory_listing`
Für Server, die ein HTML-Verzeichnislisting anbieten (Apache/nginx-„Index of“). Ab `base_url`
werden die `steps` nacheinander abgearbeitet:

- `{ "path": "Workstation/x86_64/iso/" }` hängt einen festen Pfad an.
- `{ "match": "^(?P<version>\\d+)/$" }` sucht alle Links, die zur Regex passen, und wählt den
  mit der **höchsten Version** (Gruppe `version`, numerisch verglichen: `24.10` > `24.4`).
  Mit `"select": "lexical"` wird stattdessen alphabetisch gewählt.

Beispiel Fedora: `releases/` → höchste Nummer `44/` → `Workstation/x86_64/iso/` → ISO-Datei.

Ist der höchste Treffer eine Sackgasse (z. B. ein schon angelegtes, aber noch leeres
Verzeichnis `45/`), wird der nächstniedrigere probiert.

### `json_api`
Für Hersteller mit API (Pop!_OS). Felder werden per JSON-Pointer gelesen (`/url`, `/sha_sum`).

### `github_release`
Neuestes Release eines GitHub-Repos, Asset per Regex.

### `manual`
Das Image lädt der Nutzer selbst herunter (Windows 10/11, siehe Architektur E36). BootForge öffnet
nur `download_page` im Browser. `filename_pattern` beschreibt, wie die Datei typischerweise heißt.

```json
{
  "type": "manual",
  "download_page": "https://www.microsoft.com/de-de/software-download/windows11",
  "filename_pattern": "(?i)^win.*11.*x64.*\\.iso$"
}
```

Eine `manual`-Quelle braucht `"checksum": {"type": "page_listing"}` und darf keine `signature`
haben.

## `checksum`

```json
{ "type": "file", "url": "SHA256SUMS", "algorithm": "sha256" }
{ "type": "file", "url": "{filename}.sha256", "algorithm": "sha256" }
{ "type": "file", "match": "^Fedora-Workstation-.*-CHECKSUM$", "algorithm": "sha256" }
{ "type": "from_source", "algorithm": "sha256" }
{ "type": "page_listing", "url": "https://www.microsoft.com/en-us/software-download/windows11", "algorithm": "sha256" }
```

Relative URLs beziehen sich auf das Verzeichnis des Images. Platzhalter: `{filename}`, `{version}`.

`page_listing` (nur mit `manual`): Der Hash der ausgewählten Datei muss auf dieser HTTPS-Seite
stehen. Gezählt werden nur Hex-Werte mit genau der passenden Länge (64 Zeichen bei SHA-256).
Steht gar kein Wert auf der Seite, ist das ein Fehler.

## Kategorie `other`

Die Kategorie mit der ID `other` („Eigenes Image“) braucht keine Einträge. Die App zeigt darunter
immer den festen Eintrag „Eigenes Image“ (beliebige Datei + Prüfsumme des Herstellers).

## `signature`

```json
{
  "type": "gpg_detached",
  "signs": "checksum_file",
  "url": "SHA256SUMS.gpg",
  "fingerprints": ["843938DF228D22F7B3742BC0D94AA3F0EFE21092"],
  "keyring_url": "https://keyserver.ubuntu.com/pks/lookup?op=get&search=0x8439…"
}
```

Nur wenn der Hersteller noch das veraltete v3-Signaturformat verwendet (openSUSE), zusätzlich
`"allow_v3": true` setzen. Das erzeugt bewusst eine Warnung.

**Die Fingerprints sind der Vertrauensanker.** Selbst wenn jemand eine ISO samt Prüfsummendatei
austauscht, kann er ohne den privaten Schlüssel des Herstellers keine gültige Signatur erzeugen.
Ein neuer Fingerprint darf deshalb nur eingetragen werden, wenn er von einer offiziellen Seite des
Herstellers stammt (nicht aus der Signaturdatei selbst – die könnte ja gefälscht sein).

## Regeln (werden beim Laden geprüft)

- Alle absoluten URLs müssen **HTTPS** sein.
- IDs: nur `a-z`, `0-9`, `-`; Varianten-IDs sind katalogweit eindeutig.
- Jeder Text braucht mindestens `de`.
- Jede Variante braucht eine Prüfsumme. `md5` ist erlaubt, erzeugt aber eine Warnung.
- Unbekannte Felder sind Fehler (Tippfehler wie `"chekcsum"` fallen so sofort auf).
