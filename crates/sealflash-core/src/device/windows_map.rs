//! WMI-Daten (Windows) → [`RawDrive`]. Reiner Code ohne Windows-Aufrufe, damit er auf jeder
//! Plattform getestet werden kann. Die eigentliche Abfrage steht in `windows.rs`.

use super::{Bus, RawDrive};

/// Die Felder aus `MSFT_Disk` (Namespace `ROOT\Microsoft\Windows\Storage`), die wir brauchen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WinDisk {
    pub number: u32,
    pub friendly_name: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size: u64,
    /// Siehe <https://learn.microsoft.com/windows-hardware/drivers/storage/msft-disk>: 7 = USB.
    pub bus_type: u16,
    /// Enthält die Systempartition (EFI/Bootmanager).
    pub is_system: bool,
    /// Enthält das laufende Windows.
    pub is_boot: bool,
    pub is_read_only: bool,
    pub is_offline: bool,
    /// Enthält die Auslagerungsdatei.
    pub has_pagefile: bool,
    /// Aus `Win32_DiskDrive.MediaType`, z. B. „Removable Media“.
    pub media_type: Option<String>,
    /// Laufwerksbuchstaben der Partitionen, z. B. `["E:"]`.
    pub letters: Vec<String>,
}

pub fn bus_from_type(t: u16) -> Bus {
    match t {
        7 => Bus::Usb,
        3 | 11 => Bus::Sata,
        17 => Bus::Nvme,
        12 | 13 => Bus::Mmc,
        1 | 10 => Bus::Scsi,
        14..=16 => Bus::Virtual,
        _ => Bus::Other,
    }
}

/// `system_drive` ist `%SystemDrive%` (normalerweise `C:`).
pub fn map_disk(d: &WinDisk, system_drive: &str) -> RawDrive {
    let system = if d.is_boot {
        Some("enthält das laufende Windows".to_owned())
    } else if d.is_system {
        Some("enthält die Systempartition (Bootmanager)".to_owned())
    } else if d.has_pagefile {
        Some("enthält die Auslagerungsdatei".to_owned())
    } else {
        None
    };

    // Zweite, von den Flags unabhängige Absicherung: Liegt das Systemlaufwerk hier drauf?
    let sys = system_drive.trim_end_matches('\\').to_ascii_uppercase();
    let hosts_system_drive = d
        .letters
        .iter()
        .any(|l| l.trim_end_matches('\\').eq_ignore_ascii_case(&sys));

    let removable = d
        .media_type
        .as_deref()
        .is_some_and(|m| m.eq_ignore_ascii_case("Removable Media"));
    // FriendlyName ist meist Hersteller + Modell in einem („SanDisk Ultra USB 3.0“).
    let model = d.friendly_name.clone().or_else(|| d.model.clone());

    RawDrive {
        id: format!(r"\\.\PhysicalDrive{}", d.number),
        vendor: d.manufacturer.clone().filter(|m| !m.trim().is_empty()),
        model,
        serial: d
            .serial
            .clone()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        size: d.size,
        bus: bus_from_type(d.bus_type),
        removable,
        read_only: d.is_read_only,
        system,
        busy: d.is_offline.then(|| "Datenträger ist offline".to_owned()),
        mounts: d.letters.clone(),
        platform_ok: !hosts_system_drive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{Rejection, ScanOptions, classify};

    fn usb_stick() -> WinDisk {
        WinDisk {
            number: 2,
            friendly_name: Some("SanDisk Ultra USB 3.0".into()),
            size: 32_000_000_000,
            bus_type: 7,
            media_type: Some("Removable Media".into()),
            letters: vec!["E:".into()],
            ..Default::default()
        }
    }

    fn verdict(d: WinDisk) -> Option<Rejection> {
        classify(&map_disk(&d, "C:"), ScanOptions::default()).rejection
    }

    #[test]
    fn usb_stick_eligible() {
        let r = map_disk(&usb_stick(), "C:");
        assert_eq!(r.id, r"\\.\PhysicalDrive2");
        assert!(r.removable);
        assert_eq!(verdict(usb_stick()), None);
    }

    #[test]
    fn windows_disk_rejected() {
        let c = WinDisk {
            number: 0,
            bus_type: 17,
            is_boot: true,
            is_system: true,
            letters: vec!["C:".into()],
            ..usb_stick()
        };
        assert!(matches!(verdict(c), Some(Rejection::System(r)) if r.contains("laufende Windows")));
    }

    #[test]
    fn windows_to_go_on_usb_rejected() {
        // Windows läuft von einem USB-Laufwerk → BusType 7, aber IsBoot
        assert!(matches!(
            verdict(WinDisk {
                is_boot: true,
                ..usb_stick()
            }),
            Some(Rejection::System(_))
        ));
    }

    #[test]
    fn system_drive_letter_is_second_safety_net() {
        // Angenommen, Windows meldet IsBoot/IsSystem fälschlich nicht: C: auf dem Laufwerk reicht.
        let d = WinDisk {
            letters: vec!["c:\\".into()],
            ..usb_stick()
        };
        assert!(matches!(verdict(d), Some(Rejection::Unexpected(_))));
    }

    #[test]
    fn pagefile_offline_readonly() {
        assert!(matches!(
            verdict(WinDisk {
                has_pagefile: true,
                ..usb_stick()
            }),
            Some(Rejection::System(_))
        ));
        assert!(matches!(
            verdict(WinDisk {
                is_offline: true,
                ..usb_stick()
            }),
            Some(Rejection::InUse(_))
        ));
        assert_eq!(
            verdict(WinDisk {
                is_read_only: true,
                ..usb_stick()
            }),
            Some(Rejection::ReadOnly)
        );
    }

    #[test]
    fn bus_types() {
        assert_eq!(
            verdict(WinDisk {
                bus_type: 11,
                ..usb_stick()
            }),
            Some(Rejection::NotUsb(Bus::Sata))
        );
        assert_eq!(
            verdict(WinDisk {
                bus_type: 17,
                ..usb_stick()
            }),
            Some(Rejection::NotUsb(Bus::Nvme))
        );
        assert_eq!(
            verdict(WinDisk {
                bus_type: 12,
                ..usb_stick()
            }),
            Some(Rejection::NotUsb(Bus::Mmc))
        );
        assert_eq!(
            verdict(WinDisk {
                bus_type: 15,
                ..usb_stick()
            }),
            Some(Rejection::NotUsb(Bus::Virtual))
        );
    }

    #[test]
    fn external_hdd_media_type() {
        let hdd = WinDisk {
            size: 2_000_000_000_000,
            media_type: Some("External hard disk media".into()),
            ..usb_stick()
        };
        assert_eq!(verdict(hdd), Some(Rejection::ExternalHdd));
    }
}
