//! Laufwerke erkennen und entscheiden, ob sie beschrieben werden dürfen.
//!
//! **Das ist der gefährlichste Teil von BootForge.** Ein Fehler hier kann die Systemplatte löschen.
//! Deshalb ist die Entscheidung in zwei Teile getrennt:
//!
//! 1. Plattform-Code (`linux.rs`, `windows.rs`) sammelt nur **Rohdaten** ([`RawDrive`]).
//! 2. [`classify`] entscheidet anhand fester Regeln – für alle Plattformen gleich, ohne Systemzugriff,
//!    deshalb vollständig testbar.
//!
//! Ein Laufwerk ist nur dann wählbar, wenn **alle** Regeln erfüllt sind („Default deny“).
//! Unbekannt heißt: nicht erlaubt.

use serde::Serialize;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod windows;
pub mod windows_map;

/// Größer als das ist nie ein USB-Stick, sondern eine Festplatte – auch mit Häkchen nicht wählbar.
pub const HARD_MAX_SIZE: u64 = 2 * 1000 * 1000 * 1000 * 1000; // 2 TB
/// Ab dieser Größe gilt ein nicht als „Wechselmedium“ gemeldetes USB-Laufwerk als externe Festplatte.
pub const EXTERNAL_HDD_THRESHOLD: u64 = 128 * 1000 * 1000 * 1000; // 128 GB

/// Wie ist das Laufwerk angeschlossen?
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bus {
    Usb,
    Sata,
    Nvme,
    Mmc,
    Scsi,
    Virtio,
    Virtual,
    Other,
}

/// Rohdaten eines Laufwerks, wie das Betriebssystem sie liefert.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RawDrive {
    /// Pfad zum Schreiben: `/dev/sdb` bzw. `\\.\PhysicalDrive2`.
    pub id: String,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    /// Größe in Bytes (0 = kein Medium, z. B. leerer Kartenleser).
    pub size: u64,
    pub bus: Bus,
    /// Meldet sich das Gerät als Wechselmedium? (Viele moderne Sticks tun das NICHT.)
    pub removable: bool,
    pub read_only: bool,
    /// Grund, falls das Laufwerk das laufende System enthält (z. B. „eingebunden als /“).
    pub system: Option<String>,
    /// Grund, falls es anderweitig belegt ist (LVM, RAID, Verschlüsselung).
    pub busy: Option<String>,
    /// Einhängepunkte bzw. Laufwerksbuchstaben der Partitionen.
    pub mounts: Vec<String>,
    /// Zusätzliche Plattform-Plausibilitätsprüfung bestanden? (Linux: Gerätename `sdX`.)
    pub platform_ok: bool,
}

/// Warum ein Laufwerk nicht wählbar ist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub enum Rejection {
    /// Enthält das laufende System. Wichtigste Regel – wird immer zuerst gemeldet.
    System(String),
    /// Kein USB-Gerät (interne Platte, NVMe, virtuell …).
    NotUsb(Bus),
    /// Fällt durch die zusätzliche Plattform-Prüfung.
    Unexpected(String),
    ReadOnly,
    InUse(String),
    /// Kein Medium eingelegt.
    NoMedia,
    /// Über der festen Obergrenze.
    TooLarge,
    /// Vermutlich externe Festplatte – nur mit ausdrücklicher Option sichtbar.
    ExternalHdd,
}

/// Ein Laufwerk mit Urteil.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Drive {
    pub id: String,
    /// Anzeigename, z. B. „SanDisk Ultra“.
    pub name: String,
    pub serial: Option<String>,
    pub size: u64,
    pub bus: Bus,
    pub removable: bool,
    pub mounts: Vec<String>,
    /// `None` = darf beschrieben werden.
    pub rejection: Option<Rejection>,
}

impl Drive {
    pub fn eligible(&self) -> bool {
        self.rejection.is_none()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanOptions {
    /// Große USB-Laufwerke ohne Wechselmedium-Kennung (externe Festplatten) anzeigen.
    pub show_external_hdds: bool,
}

/// Die Regeln. Reihenfolge = Priorität der Meldung (die wichtigste Begründung gewinnt).
pub fn classify(raw: &RawDrive, opts: ScanOptions) -> Drive {
    let rejection = if let Some(reason) = &raw.system {
        Some(Rejection::System(reason.clone()))
    } else if raw.bus != Bus::Usb {
        Some(Rejection::NotUsb(raw.bus.clone()))
    } else if !raw.platform_ok {
        Some(Rejection::Unexpected(raw.id.clone()))
    } else if raw.read_only {
        Some(Rejection::ReadOnly)
    } else if let Some(reason) = &raw.busy {
        Some(Rejection::InUse(reason.clone()))
    } else if raw.size == 0 {
        Some(Rejection::NoMedia)
    } else if raw.size > HARD_MAX_SIZE {
        Some(Rejection::TooLarge)
    } else if !raw.removable && raw.size > EXTERNAL_HDD_THRESHOLD && !opts.show_external_hdds {
        Some(Rejection::ExternalHdd)
    } else {
        None
    };

    Drive {
        id: raw.id.clone(),
        name: display_name(raw),
        serial: raw.serial.clone(),
        size: raw.size,
        bus: raw.bus.clone(),
        removable: raw.removable,
        mounts: raw.mounts.clone(),
        rejection,
    }
}

fn display_name(raw: &RawDrive) -> String {
    let parts: Vec<&str> = [raw.vendor.as_deref(), raw.model.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    // „SanDisk SanDisk Ultra“ vermeiden.
    match parts.as_slice() {
        [v, m] if m.to_lowercase().starts_with(&v.to_lowercase()) => m.to_string(),
        [] => raw.id.clone(),
        p => p.join(" "),
    }
}

/// Alle Laufwerke des Systems, wählbare zuerst.
pub fn list(opts: ScanOptions) -> std::io::Result<Vec<Drive>> {
    Ok(judge(scan_raw()?, opts))
}

/// Wie [`list`], aber mit einem anderen Wurzelverzeichnis statt `/` (nachgebautes `/sys`).
/// Nur für Entwicklung und Tests – die App nutzt es ausschließlich in Debug-Builds.
#[cfg(target_os = "linux")]
pub fn list_from_root(root: &std::path::Path, opts: ScanOptions) -> std::io::Result<Vec<Drive>> {
    Ok(judge(linux::scan(root)?, opts))
}

fn judge(raw: Vec<RawDrive>, opts: ScanOptions) -> Vec<Drive> {
    let mut drives: Vec<Drive> = raw.iter().map(|r| classify(r, opts)).collect();
    drives.sort_by(|a, b| {
        b.eligible()
            .cmp(&a.eligible())
            .then_with(|| a.id.cmp(&b.id))
    });
    drives
}

#[cfg(target_os = "linux")]
fn scan_raw() -> std::io::Result<Vec<RawDrive>> {
    linux::scan(std::path::Path::new("/"))
}

#[cfg(windows)]
fn scan_raw() -> std::io::Result<Vec<RawDrive>> {
    windows::scan()
}

#[cfg(not(any(target_os = "linux", windows)))]
fn scan_raw() -> std::io::Result<Vec<RawDrive>> {
    Err(std::io::Error::other(
        "Laufwerkserkennung wird auf diesem System nicht unterstützt",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1000 * 1000 * 1000;

    /// Ein ganz normaler, wählbarer 32-GB-Stick. Tests verändern je ein Merkmal.
    fn stick() -> RawDrive {
        RawDrive {
            id: "/dev/sdb".into(),
            vendor: Some("SanDisk".into()),
            model: Some("SanDisk Ultra".into()),
            serial: Some("4C530001".into()),
            size: 32 * GB,
            bus: Bus::Usb,
            removable: true,
            read_only: false,
            system: None,
            busy: None,
            mounts: vec!["/media/user/STICK".into()],
            platform_ok: true,
        }
    }

    fn verdict(r: RawDrive) -> Option<Rejection> {
        classify(&r, ScanOptions::default()).rejection
    }

    #[test]
    fn normal_stick_is_eligible() {
        let d = classify(&stick(), ScanOptions::default());
        assert!(d.eligible());
        assert_eq!(d.name, "SanDisk Ultra");
    }

    #[test]
    fn fixed_flag_small_stick_is_still_eligible() {
        // Viele Sticks melden sich NICHT als Wechselmedium – unter 128 GB trotzdem erlaubt.
        assert_eq!(
            verdict(RawDrive {
                removable: false,
                size: 64 * GB,
                ..stick()
            }),
            None
        );
    }

    #[test]
    fn system_disk_always_rejected_even_if_usb() {
        // z. B. Linux läuft gerade vom USB-Stick
        let r = RawDrive {
            system: Some("eingebunden als /".into()),
            ..stick()
        };
        assert_eq!(
            verdict(r),
            Some(Rejection::System("eingebunden als /".into()))
        );
    }

    #[test]
    fn system_wins_over_everything_else() {
        let r = RawDrive {
            system: Some("Boot".into()),
            bus: Bus::Nvme,
            read_only: true,
            ..stick()
        };
        assert!(matches!(verdict(r), Some(Rejection::System(_))));
    }

    #[test]
    fn internal_buses_rejected() {
        for bus in [
            Bus::Sata,
            Bus::Nvme,
            Bus::Mmc,
            Bus::Scsi,
            Bus::Virtio,
            Bus::Virtual,
            Bus::Other,
        ] {
            assert_eq!(
                verdict(RawDrive {
                    bus: bus.clone(),
                    ..stick()
                }),
                Some(Rejection::NotUsb(bus))
            );
        }
    }

    #[test]
    fn platform_check_rejects() {
        assert!(matches!(
            verdict(RawDrive {
                platform_ok: false,
                ..stick()
            }),
            Some(Rejection::Unexpected(_))
        ));
    }

    #[test]
    fn read_only_busy_and_empty_rejected() {
        assert_eq!(
            verdict(RawDrive {
                read_only: true,
                ..stick()
            }),
            Some(Rejection::ReadOnly)
        );
        assert!(matches!(
            verdict(RawDrive {
                busy: Some("LVM".into()),
                ..stick()
            }),
            Some(Rejection::InUse(_))
        ));
        assert_eq!(
            verdict(RawDrive { size: 0, ..stick() }),
            Some(Rejection::NoMedia)
        );
    }

    #[test]
    fn external_hdd_hidden_unless_option() {
        let hdd = RawDrive {
            removable: false,
            size: 1000 * GB,
            ..stick()
        };
        assert_eq!(verdict(hdd.clone()), Some(Rejection::ExternalHdd));
        assert_eq!(
            classify(
                &hdd,
                ScanOptions {
                    show_external_hdds: true
                }
            )
            .rejection,
            None
        );
    }

    #[test]
    fn hard_size_limit_ignores_option() {
        let huge = RawDrive {
            removable: false,
            size: 4000 * GB,
            ..stick()
        };
        assert_eq!(
            classify(
                &huge,
                ScanOptions {
                    show_external_hdds: true
                }
            )
            .rejection,
            Some(Rejection::TooLarge)
        );
        // auch wenn es sich als Wechselmedium ausgibt
        assert_eq!(
            verdict(RawDrive {
                size: 3000 * GB,
                ..stick()
            }),
            Some(Rejection::TooLarge)
        );
    }

    #[test]
    fn display_names() {
        let n = |v: Option<&str>, m: Option<&str>| {
            display_name(&RawDrive {
                vendor: v.map(Into::into),
                model: m.map(Into::into),
                ..stick()
            })
        };
        assert_eq!(
            n(Some("Kingston"), Some("DataTraveler 3.0")),
            "Kingston DataTraveler 3.0"
        );
        assert_eq!(
            n(Some("SanDisk "), Some("SanDisk 3.2Gen1")),
            "SanDisk 3.2Gen1"
        );
        assert_eq!(n(None, None), "/dev/sdb");
        assert_eq!(n(Some("  "), Some("Flash Disk")), "Flash Disk");
    }
}
