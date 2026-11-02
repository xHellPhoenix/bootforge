# Checkliste: USB-Erkennung mit echter Hardware testen

Die automatischen Tests prüfen die Regeln mit nachgebauten Systemdaten. Ob die Erkennung auf
**echter Hardware** stimmt, kann nur ein Test an einem richtigen Rechner zeigen. Bis Phase 5 wird
**nichts geschrieben**, die Tests hier sind also gefahrlos.

## Vorbereitung

```bash
git clone https://github.com/xHellPhoenix/sealflash && cd sealflash
npm install
```

Am schnellsten geht es ohne GUI mit dem Werkzeug `list_drives`. Es zeigt jedes Laufwerk mit Urteil:

```bash
cargo run -p sealflash-core --example list_drives          # Standard
cargo run -p sealflash-core --example list_drives -- --hdd  # inkl. externer Festplatten
```

In der App: `npm run tauri dev`, dann unten auf „Stick wählen …“.

## Fälle

Bitte jeweils notieren, was angezeigt wird. Wichtig sind vor allem die **Begründungen** bei
ausgeblendeten Laufwerken.

| # | Aufbau | Erwartung |
|---|--------|-----------|
| 1 | Kein Stick eingesteckt | Keine wählbaren Laufwerke. Interne Platte(n) ausgeblendet als „Systemlaufwerk“ bzw. „Kein USB-Laufwerk“ |
| 2 | Ein normaler USB-Stick (≤ 128 GB) | Genau dieser Stick wählbar, mit korrektem Namen und korrekter Größe |
| 3 | Stick einstecken, **während** der Dialog offen ist | Erscheint nach spätestens 2 s |
| 4 | Stick abziehen, während der Dialog offen ist | Verschwindet; war er gewählt, wird die Auswahl aufgehoben |
| 5 | Stick mit eingebundener Partition (im Dateimanager geöffnet) | Wählbar, „eingebunden: …“ bzw. Laufwerksbuchstabe wird angezeigt |
| 6 | Zu kleiner Stick (z. B. 2 GB für Ubuntu) | Erscheint, ist aber gesperrt: „Zu klein – das Image braucht …“ |
| 7 | Externe USB-Festplatte | Ausgeblendet als „Externe Festplatte“. Mit dem Häkchen sichtbar und wählbar |
| 8 | SD-Karte im **eingebauten** Kartenleser des Laptops | Ausgeblendet (Linux: „Kein USB-Laufwerk (mmc)“) |
| 9 | SD-Karte im **USB**-Kartenleser | Wählbar wie ein Stick |
| 10 | USB-Kartenleser **ohne** Karte | Ausgeblendet als „Kein Medium eingelegt“ |
| 11 | Stick mit Schreibschutzschalter (falls vorhanden), Schalter an | Ausgeblendet als „Schreibgeschützt“ |
| 12 | Linux: Rechner von einem Live-USB-Stick gebootet | Dieser Stick ist ausgeblendet als „Systemlaufwerk – … als / eingebunden“ |
| 13 | Linux: Stick mit LUKS-Verschlüsselung, entsperrt | Ausgeblendet als „Wird verwendet“ |

## Rückmeldung

Für einen Fall, der nicht wie erwartet läuft, brauche ich:

- **Linux:** Ausgabe von `list_drives`, außerdem `lsblk -o NAME,TRAN,RM,RO,SIZE,MOUNTPOINTS,MODEL`
  und `readlink -f /sys/block/sdX` für das betroffene Gerät.
- **Windows** (PowerShell): Ausgabe von `list_drives` und
  `Get-Disk | Format-List Number,FriendlyName,BusType,IsSystem,IsBoot,IsReadOnly,Size`.
