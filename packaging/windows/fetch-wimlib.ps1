# Lädt wimlib (für das Aufteilen von install.wim > 4 GB) und legt die benötigten Dateien nach
# packaging/windows/wimlib/. Wird vor `tauri build` unter Windows automatisch ausgeführt
# (siehe src-tauri/tauri.windows-release.conf.json).
#
# Sicherheit: wimlib veröffentlicht keine Prüfsummen. Der SHA-256 unten wurde beim ersten Download
# festgehalten („Trust on first use“) – jeder spätere Download muss exakt dazu passen, sonst bricht
# der Build ab. Für ein Update: neue Version laden, Hash prüfen, beide Werte hier ändern.
$ErrorActionPreference = 'Stop'

$Version = '1.14.5'
$Sha256  = '2f446d6fa3866582175f1a22a7be198eeee0aec7aba5b4e04ad25c99eae2d265'
$Url     = "https://wimlib.net/downloads/wimlib-$Version-windows-x86_64-bin.zip"

$Dest = Join-Path $PSScriptRoot 'wimlib'
$Marker = Join-Path $Dest ".version-$Version"
if (Test-Path $Marker) {
    Write-Host "wimlib $Version ist schon vorhanden."
    exit 0
}

$Zip = Join-Path ([IO.Path]::GetTempPath()) "wimlib-$Version.zip"
Write-Host "Lade $Url"
Invoke-WebRequest -Uri $Url -OutFile $Zip -UseBasicParsing

$Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
if ($Actual -ne $Sha256) {
    Remove-Item $Zip
    throw "wimlib: Prüfsumme stimmt nicht! erwartet $Sha256, ist $Actual"
}

$Tmp = Join-Path ([IO.Path]::GetTempPath()) "wimlib-$Version"
if (Test-Path $Tmp) { Remove-Item -Recurse -Force $Tmp }
Expand-Archive -Path $Zip -DestinationPath $Tmp
if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
New-Item -ItemType Directory -Path $Dest | Out-Null
foreach ($f in 'wimlib-imagex.exe', 'libwim-15.dll', 'COPYING.txt', 'COPYING.GPLv3.txt', 'COPYING.LGPL.txt', 'COPYING.libdivsufsort-lite.txt') {
    Copy-Item (Join-Path $Tmp $f) $Dest
}
New-Item -ItemType File -Path $Marker | Out-Null
Remove-Item $Zip
Remove-Item -Recurse -Force $Tmp
Write-Host "wimlib $Version geprüft und nach $Dest gelegt."
