# Anleitung: Den ersten echten Stick schreiben (Windows)

> ⚠ **Der Stick wird komplett gelöscht.** Nimm einen, auf dem nichts Wichtiges liegt.
> Ziehe sicherheitshalber alle anderen USB-Laufwerke ab (externe Festplatten, andere Sticks).

Es gibt zwei Wege. **Weg A** braucht nur Rust (das du schon installiert hast), **Weg B** testet die
richtige App mit Oberfläche und braucht zusätzlich Node.js.

## Weg A: ohne Oberfläche (empfohlen für den ersten Test)

1. **PowerShell als Administrator öffnen**: Startmenü → „PowerShell“ → Rechtsklick →
   „Als Administrator ausführen“. Zum Schreiben auf einen Datenträger verlangt Windows Adminrechte.
2. In den Projektordner wechseln und den neuesten Stand holen:
   ```powershell
   cd $HOME\bootforge
   git fetch
   git checkout phase-5-flash
   ```
3. Nachsehen, welche Nummer dein Stick hat:
   ```powershell
   cargo run --release -p bootforge-core --example list_drives
   ```
   Beispiel: `\\.\PhysicalDrive1  8.0 GB  Generic Flash Disk  ✔ WÄHLBAR` → Nummer **1**.
4. Debian netinst (≈ 790 MB) auf den Stick schreiben:
   ```powershell
   cargo run --release -p bootforge-core --example forge -- debian-netinst 1
   ```
   Das Programm zeigt den Stick und fragt nach seinem Namen (z. B. `Generic Flash Disk`).
   Erst wenn du ihn richtig eintippst, wird geschrieben.

`--release` ist wichtig: Die optimierte Version hasht und schreibt deutlich schneller.

### Was du sehen solltest

```text
▶ Image beschaffen und prüfen
  · Checksum
  · Signature
    ✔ signiert von Debian CD signing key <debian-cd@lists.debian.org>
  · Download   100.0 %  …
  · Verify
▶ Auf \\.\PhysicalDrive1 schreiben
  · CheckDrive
  · VerifyImage
  · Prepare
  · Write      100.0 %  …
  · Readback   100.0 %  …
✔ Fertig in …: 792.7 MB geschrieben und durch Zurücklesen bestätigt
```

Danach zeigt Windows evtl. einen neuen Datenträger oder meldet „Laufwerk muss formatiert werden“.
**Auf „Abbrechen“ klicken, nicht formatieren!** Das ist normal: Windows versteht das Linux-Dateisystem
auf dem Stick nicht. Die App weist nach dem Schreiben unter Windows auch selbst darauf hin.

## Weg B: mit Oberfläche

Zusätzlich [Node.js LTS](https://nodejs.org) installieren, dann in einer **Administrator**-PowerShell:

```powershell
cd $HOME\bootforge
npm install
npm run tauri dev
```

Debian → Netinst → „Stick wählen …“ → deinen Stick → „Forge!“ → Namen eintippen → „Löschen und schreiben“.

(In der fertigen App fragt Windows beim Start automatisch nach Adminrechten. Beim Entwickeln mit
`tauri dev` passiert das nicht, deshalb hier die Administrator-PowerShell.)

## Danach prüfen

1. Stick in einen Rechner stecken, im BIOS/UEFI-Bootmenü (oft F12, F11 oder Esc beim Start) auswählen.
2. Der Debian-Installer sollte starten. Du musst nichts installieren, einfach wieder ausschalten.

## Wenn etwas schiefgeht

Bitte die komplette Ausgabe kopieren. Zusätzlich hilfreich:

```powershell
Get-Disk | Format-List Number,FriendlyName,BusType,IsSystem,IsBoot,IsReadOnly,IsOffline,Size
Get-Partition -DiskNumber 1 | Format-List PartitionNumber,DriveLetter,Size,Type
```

Bekannte Meldungen:

| Meldung | Bedeutung |
|---|---|
| `Administratorrechte nötig` | PowerShell war nicht „Als Administrator“ gestartet |
| `… lässt sich nicht sperren` | Ein Programm (Explorer-Fenster, Virenscanner) greift noch auf den Stick zu. Schließen und erneut versuchen |
| `Laufwerk hat sich geändert` | Stick wurde zwischendurch abgezogen oder ein anderer eingesteckt. Nichts wurde geschrieben |
| `Prüfung fehlgeschlagen … ab Byte …` | Der Stick speichert nicht korrekt, ist also defekt oder eine Fälschung |
