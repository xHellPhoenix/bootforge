# Ein Release bauen

## Ablauf

1. Version an **drei** Stellen gleich setzen:
   - `Cargo.toml` → `[workspace.package] version`
   - `package.json` → `version`
   - `src-tauri/tauri.conf.json` → `version`
2. `python3 scripts/third_party.py > THIRD-PARTY.md` (Lizenzliste aktualisieren), committen.
3. Tag setzen und hochladen:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. Der Workflow **Release** (`.github/workflows/release.yml`) baut auf GitHub:
   - Windows: `…_x64-setup.exe` (NSIS) und `…_x64_de-DE.msi` / `…_en-US.msi`,
   - Linux (Ubuntu 22.04): `…_amd64.deb` und `….x86_64.rpm`,
   - lädt den wimlib-Quelltext dazu (GPL),
   - erzeugt `SHA256SUMS.txt` über alle Dateien.
5. Unter *Releases* liegt danach ein **Entwurf**. Kurz prüfen (Dateien da? Installer startet?),
   Text ergänzen und auf „Publish release“ klicken.
6. Katalog-Repo: Änderungen, die neue Felder/Typen brauchen, **gleichzeitig mit der Veröffentlichung**
   mergen. Ältere App-Versionen lehnen unbekannte Felder ab, die neue App lehnt veraltete ab.
   Beide zeigen dann den Hinweis „passt nicht zu dieser Version“ und nehmen ihre gespeicherte Liste.
   **Für 0.1.0:** Es gibt noch keine ältere App, also den Zweig `windows-manual-other` beim
   Veröffentlichen nach `main` mergen.

## Lokal bauen

```bash
npm ci
npx tauri build                                                  # Linux: .deb + .rpm
npx tauri build --config src-tauri/tauri.windows-release.conf.json   # Windows: .exe + .msi mit wimlib
```

Die Pakete liegen dann unter `target/release/bundle/`. Plattformabhängige Einstellungen stehen in
`src-tauri/tauri.linux.conf.json` und `src-tauri/tauri.windows.conf.json`. Tauri legt sie
automatisch über `tauri.conf.json`.

- **Linux:** `beforeBuildCommand` baut zusätzlich den Root-Helper. Das .deb/.rpm legt ihn nach
  `/usr/lib/bootforge/` und die polkit-Regel nach `/usr/share/polkit-1/actions/`.
- **Windows:** Die Zusatz-Konfiguration `tauri.windows-release.conf.json` führt vorher
  `packaging/windows/fetch-wimlib.ps1` aus. Sie ist bewusst getrennt: Tauri prüft eingetragene
  Ressourcen bei *jedem* Kompilieren. Stünde wimlib in der normalen Konfiguration, würden
  `cargo test` und `tauri dev` ohne vorherigen Download scheitern. Das Skript lädt
  wimlib 1.14.5 und bricht ab, wenn der SHA-256 nicht passt. `wimlib-imagex.exe` und
  `libwim-15.dll` landen im Installationsordner neben `bootforge.exe`, die Lizenztexte unter
  `licenses\`.

## wimlib aktualisieren

1. Neue Version von <https://wimlib.net/downloads/> laden, SHA-256 berechnen
   (`Get-FileHash` bzw. `sha256sum`). Am besten von zwei verschiedenen Rechnern/Netzen prüfen.
   (Siehe auch `scripts`: die Version steht in mehreren Dateien, `grep -rn 1.14.5` findet alle.)
2. Version und Hash in `packaging/windows/fetch-wimlib.ps1` und für den Quelltext in
   `.github/workflows/release.yml` ändern, außerdem die Versionsangabe in `scripts/third_party.py`.
