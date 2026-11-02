# Anleitung: Windows-11-Stick und „Eigenes Image“ testen

> ⚠ **Der Stick wird komplett gelöscht.** Nimm einen mit mindestens 8 GB, auf dem nichts Wichtiges
> liegt. Ziehe sicherheitshalber alle anderen USB-Laufwerke ab.

Getestet wird die App mit Oberfläche. Sie muss unter Windows mit **Administratorrechten** laufen.
Im Release-Build sorgt das Manifest dafür. Beim Entwickeln (`tauri dev`) startest du dafür das
Terminal als Administrator.

## 1. Vorbereiten (einmalig)

1. **PowerShell als Administrator** öffnen (Startmenü → „PowerShell“ → Rechtsklick →
   „Als Administrator ausführen“).
2. Neuesten Stand holen:
   ```powershell
   cd $HOME\sealflash
   git fetch
   git checkout phase-6-windows
   npm install
   ```
3. **wimlib** besorgen. Die Datei `install.wim` in Windows-11-ISOs ist größer als 4 GB. FAT32 kann
   so große Dateien nicht speichern, deshalb teilt SealFlash sie mit `wimlib-imagex` auf (in Phase 7
   liegt das Programm der App bei).
   - Von <https://wimlib.net/downloads/> die Windows-Version (x86_64, „binary“) herunterladen und
     entpacken.
   - `wimlib-imagex.exe` **und** `libwim-15.dll` nach `sealflash\target\debug\` kopieren (neben
     `sealflash.exe`; den Ordner gibt es nach dem ersten Start). Alternativ den entpackten Ordner
     zum `PATH` hinzufügen.
4. Katalog mit Windows-Einträgen verwenden. Er liegt noch auf einem eigenen Zweig, weil ältere
   App-Versionen die neuen Felder ablehnen würden:
   ```powershell
   $env:SEALFLASH_CATALOG_URL = "https://raw.githubusercontent.com/xHellPhoenix/sealflash-catalog/windows-manual-other/catalog.json"
   npm run tauri dev
   ```

## 2. Windows 11

1. Links **Windows** → **Windows 11** wählen.
2. **„Download-Seite öffnen“** öffnet Microsofts Seite im Browser. Dort unter „Datenträgerimage (ISO)
   herunterladen“ *Windows 11 (multi-edition ISO for x64 devices)* und die Sprache wählen, dann die
   ISO speichern (≈ 5–7 GB).
3. In SealFlash **„Datei wählen …“** und die ISO auswählen. SealFlash
   - erkennt, dass es ein Windows-Installationsmedium ist,
   - lädt Microsofts Prüfsummenliste von derselben Seite,
   - berechnet die SHA-256 der Datei (dauert ca. 30 s) und sucht sie in der Liste.

   Erwartet: **„✓ Geprüft – die Prüfsumme steht in Microsofts Liste.“**, Schreibmodus
   „Windows-Installer“.
4. Stick wählen → **Flash!** → Namen eintippen. Erwartete Schritte:
   Stick erneut prüfen → Image vor dem Schreiben prüfen → Stick vorbereiten → *install.wim
   aufteilen* → Partitionstabelle anlegen (GPT) → Mit FAT32 formatieren → Dateien kopieren →
   Zurücklesen und vergleichen.
5. Danach im Explorer: Der Stick heißt **SEALFLASH** und enthält `sources\install.swm`,
   `install2.swm` … statt `install.wim`.
6. **Booten:** Den PC per Boot-Menü (oft F12/F8/Esc) vom Stick im **UEFI-Modus** starten. Das
   Windows-Setup sollte erscheinen. Installieren musst du nicht, bis zur Sprachauswahl reicht.

### Gegenproben (sollen scheitern)
- Eine beliebige andere Datei unter „Windows 11“ auswählen (z. B. ein Linux-ISO) → „Das ist kein
  Windows-Installationsmedium“.
- Eine Windows-ISO, deren Hash nicht in der Liste steht (z. B. eine ältere Version oder eine
  veränderte Datei) → „Diese Datei steht nicht in Microsofts Prüfsummenliste …“. „Flash!“ bleibt
  gesperrt.

## 3. Eigenes Image

1. Links ganz unten **„Eigenes Image“** wählen (oder Kategorie „Eigenes Image“).
2. Eine Datei wählen, z. B. das Debian-ISO aus dem letzten Test
   (`%LOCALAPPDATA%\io.github.xhellphoenix.sealflash\cache\images\`) oder ein Raspberry-Pi-Image
   (`.img.xz`).
3. Die Prüfsumme von der Download-Seite des Herstellers einfügen (z. B. aus `SHA256SUMS`).
   Formate wie `sha256:ABC…` oder Großbuchstaben sind egal.
4. **„Prüfen“**. Erwartet: „✓ Geprüft – die Prüfsumme stimmt.“ und der passende Schreibmodus
   (1:1-Abbild oder Windows-Installer).
5. Eine Ziffer der Prüfsumme ändern und erneut prüfen → „Die Prüfsumme stimmt nicht!“.

## Bekannte Grenzen
- Der Windows-Stick startet nur im **UEFI-Modus** (seit Windows 11 Pflicht). Alte PCs mit reinem
  BIOS/CSM werden nicht unterstützt.
- Sticks mit 4096-Byte-Sektoren (selten, manche USB-Festplatten) lehnt der Windows-Modus ab.

## 4. Windows-Einrichtung anpassen (ab 0.3)

1. Windows 11 wie oben wählen und die ISO prüfen lassen.
2. Unter dem Prüfergebnis **„Windows-Einrichtung anpassen“** einschalten:
   - „Lokales Konto …“ an, Benutzername z. B. `Test`
   - Computername z. B. `SEALFLASH-TEST`
   - Region `de-DE`, Datenschutz-Fragen überspringen
   - Bei einem alten PC ohne TPM 2.0 zusätzlich „Prüfung … überspringen“
3. Ungültige Eingaben ausprobieren: Benutzername `Administrator` oder `a?b`, Computername
   `Büro-PC`. Darunter erscheint eine Fehlermeldung, „Flash!“ ist gesperrt.
4. **Flash!** → Im Bestätigungsdialog steht die Zusammenfassung unter „Windows-Einrichtung“.
5. Danach liegt auf dem Stick im Explorer die Datei `autounattend.xml`. Mit dem Editor öffnen: Sie
   enthält kein Passwort und keinen Abschnitt `DiskConfiguration`.
6. Vom Stick installieren. Erwartet:
   - Die Sprachauswahl am Anfang ist schon ausgefüllt bzw. wird übersprungen.
   - Die Festplatte wählst du **selbst** aus.
   - Nach dem Neustart fragt Windows **nicht** nach Microsoft-Konto, Region oder Datenschutz.
   - Anmeldung als `Test` ohne Passwort. Danach unter Einstellungen → Konten ein Passwort setzen.
