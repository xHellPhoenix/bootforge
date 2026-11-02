//! Laufwerke unter Windows erkennen – über WMI (Windows Management Instrumentation).
//!
//! Wir fragen drei Klassen ab:
//! - `MSFT_Disk` (Storage-Namespace): Bus-Typ, IsSystem/IsBoot, Größe, Name, Seriennummer
//! - `MSFT_Partition`: Laufwerksbuchstaben je Datenträger
//! - `Win32_DiskDrive` (klassischer Namespace): MediaType „Removable Media“
//!
//! Die Umrechnung in [`RawDrive`] steht in `windows_map.rs` und ist plattformunabhängig getestet.
//!
//! Wichtig: Eine WMI-Verbindung darf den Thread nicht wechseln (COM). Deshalb ist das hier
//! synchroner Code – die App ruft ihn in einem eigenen Thread auf (`spawn_blocking`).

use std::collections::HashMap;
use std::io;

use serde::Deserialize;
use wmi::WMIConnection;

use super::RawDrive;
use super::windows_map::{WinDisk, map_disk};

#[derive(Deserialize, Debug)]
#[serde(rename = "MSFT_Disk", rename_all = "PascalCase")]
struct MsftDisk {
    number: u32,
    friendly_name: Option<String>,
    manufacturer: Option<String>,
    model: Option<String>,
    serial_number: Option<String>,
    size: Option<u64>,
    bus_type: Option<u16>,
    is_system: Option<bool>,
    is_boot: Option<bool>,
    is_read_only: Option<bool>,
    is_offline: Option<bool>,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "MSFT_Partition", rename_all = "PascalCase")]
struct MsftPartition {
    disk_number: u32,
    access_paths: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_DiskDrive", rename_all = "PascalCase")]
struct Win32DiskDrive {
    index: u32,
    media_type: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename = "Win32_PageFileUsage", rename_all = "PascalCase")]
struct PageFile {
    name: String,
}

fn wmi_err(e: wmi::WMIError) -> io::Error {
    io::Error::other(format!("WMI-Abfrage fehlgeschlagen: {e}"))
}

pub fn scan() -> io::Result<Vec<RawDrive>> {
    let storage =
        WMIConnection::with_namespace_path(r"ROOT\Microsoft\Windows\Storage").map_err(wmi_err)?;
    let cimv2 = WMIConnection::new().map_err(wmi_err)?;

    let disks: Vec<MsftDisk> = storage.query().map_err(wmi_err)?;
    let parts: Vec<MsftPartition> = storage.query().map_err(wmi_err)?;
    let drives: Vec<Win32DiskDrive> = cimv2.query().map_err(wmi_err)?;
    // Fehlt die Info, lieber „keine Auslagerungsdatei bekannt“ – IsBoot/IsSystem greifen trotzdem.
    let pagefiles: Vec<PageFile> = cimv2.query().unwrap_or_default();

    let mut letters: HashMap<u32, Vec<String>> = HashMap::new();
    for p in parts {
        for path in p.access_paths.unwrap_or_default() {
            // „E:\“ – Volume-GUID-Pfade („\\?\Volume{…}\“) interessieren nicht.
            if path.len() <= 3 && path.chars().nth(1) == Some(':') {
                letters
                    .entry(p.disk_number)
                    .or_default()
                    .push(path.trim_end_matches('\\').to_owned());
            }
        }
    }
    let media: HashMap<u32, Option<String>> = drives
        .into_iter()
        .map(|d| (d.index, d.media_type))
        .collect();
    let pagefile_letters: Vec<String> = pagefiles
        .iter()
        .filter_map(|p| p.name.get(..2).map(str::to_ascii_uppercase))
        .collect();

    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());

    Ok(disks
        .into_iter()
        .map(|d| {
            let l = letters.remove(&d.number).unwrap_or_default();
            let has_pagefile = l
                .iter()
                .any(|x| pagefile_letters.contains(&x.to_ascii_uppercase()));
            let win = WinDisk {
                number: d.number,
                friendly_name: d.friendly_name,
                manufacturer: d.manufacturer,
                model: d.model,
                serial: d.serial_number,
                size: d.size.unwrap_or(0),
                bus_type: d.bus_type.unwrap_or(0),
                // Unbekannt → als System behandeln. Lieber ein Stick zu wenig als die Systemplatte.
                is_system: d.is_system.unwrap_or(true),
                is_boot: d.is_boot.unwrap_or(true),
                is_read_only: d.is_read_only.unwrap_or(true),
                is_offline: d.is_offline.unwrap_or(false),
                has_pagefile,
                media_type: media.get(&d.number).cloned().flatten(),
                letters: l,
            };
            map_disk(&win, &system_drive)
        })
        .collect())
}
