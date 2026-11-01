//! Laufwerke unter Linux erkennen – direkt aus `/sys` und `/proc`.
//!
//! Warum nicht `lsblk`? Dann hingen wir von dessen Version und Ausgabeformat ab, und ein fehlendes
//! Programm wäre ein Laufzeitfehler. `/sys/block` ist die stabile Kernel-Schnittstelle, die auch
//! `lsblk` selbst liest. Außerdem kann man so mit einem nachgebauten Verzeichnisbaum testen
//! (Parameter `root`).
//!
//! Was wir je Laufwerk lesen:
//!
//! ```text
//! /sys/block/sdb                → Symlink auf …/usb2/2-1/…/block/sdb   (USB? → Pfad enthält /usb)
//! /sys/block/sdb/size           → Größe in 512-Byte-Sektoren (immer 512, egal welche Sektorgröße)
//! /sys/block/sdb/removable, ro  → 0/1
//! /sys/block/sdb/dev            → „8:16“ (Major:Minor, zum Abgleich mit /proc/self/mountinfo)
//! /sys/block/sdb/sdb1/          → Partition (enthält Datei `partition`)
//! /sys/block/sdb/sdb1/holders/  → z. B. dm-0, wenn LVM/LUKS/RAID darauf liegt
//! …/usb2/2-1/{manufacturer,product,serial} → Klartextnamen aus dem USB-Gerät
//! ```

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Bus, RawDrive};

/// Einhängepunkte, die das laufende System ausmachen. Liegt eine davon auf einem Laufwerk,
/// ist es tabu – egal wie es angeschlossen ist.
const SYSTEM_MOUNTS: &[&str] = &[
    "/",
    "/boot",
    "/boot/efi",
    "/efi",
    "/usr",
    "/var",
    "/home",
    "/etc",
    "/opt",
    "/srv",
    "/nix",
    "/var/log",
];

/// Virtuelle Geräte, die gar nicht erst aufgelistet werden.
const SKIP_PREFIXES: &[&str] = &["loop", "ram", "zram", "dm-", "md", "nbd", "sr", "fd", "zd"];

fn read(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Wie `read`, aber ohne nackte Hex-Kennungen (virtio meldet als „vendor“ z. B. `0x1af4`).
fn read_name(path: impl AsRef<Path>) -> Option<String> {
    read(path).filter(|s| !s.starts_with("0x"))
}

fn read_flag(path: impl AsRef<Path>) -> bool {
    read(path).as_deref() == Some("1")
}

/// Scannt alle Blockgeräte unterhalb von `root` (im echten Betrieb `/`).
pub fn scan(root: &Path) -> io::Result<Vec<RawDrive>> {
    let mounts = parse_mountinfo(&read(root.join("proc/self/mountinfo")).unwrap_or_default());
    let swaps = parse_swaps(&read(root.join("proc/swaps")).unwrap_or_default());
    let block = root.join("sys/block");

    let mut out = Vec::new();
    for entry in fs::read_dir(&block)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP_PREFIXES.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        out.push(read_disk(root, &block.join(&name), &name, &mounts, &swaps));
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn read_disk(
    root: &Path,
    dir: &Path,
    name: &str,
    mounts: &HashMap<String, Vec<String>>,
    swaps: &[String],
) -> RawDrive {
    // Wo hängt das Gerät im Gerätebaum? Daran erkennen wir den Bus.
    let real = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_owned());
    let real_str = real.to_string_lossy();
    let bus = bus_from_path(&real_str, name);
    let usb_dev = if bus == Bus::Usb {
        find_usb_device(&real)
    } else {
        None
    };

    // Partitionen = Unterordner mit einer Datei `partition`.
    let mut parts: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("partition").exists())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    parts.sort();

    // Einhängepunkte, Swap, Holder: für das Gerät selbst UND alle Partitionen.
    let mut mountpoints = Vec::new();
    let mut system = None;
    let mut busy = None;
    let all = std::iter::once((name.to_owned(), dir.to_owned())).chain(parts);
    for (pname, pdir) in all {
        if let Some(dev) = read(pdir.join("dev")) {
            for mp in mounts.get(&dev).into_iter().flatten() {
                if SYSTEM_MOUNTS.contains(&mp.as_str()) && system.is_none() {
                    system = Some(format!("{pname} ist als {mp} eingebunden"));
                }
                mountpoints.push(mp.clone());
            }
        }
        if swaps.iter().any(|s| s == &pname) && system.is_none() {
            system = Some(format!(
                "{pname} wird als Auslagerungsspeicher (Swap) verwendet"
            ));
        }
        // Holder (dm-0, md0 …): LVM, LUKS oder RAID liegt auf dieser Partition.
        for holder in list_names(&pdir.join("holders")) {
            let holder_dev = read(root.join("sys/block").join(&holder).join("dev"));
            let holder_mounts = holder_dev.as_ref().and_then(|d| mounts.get(d));
            let critical = holder_mounts
                .into_iter()
                .flatten()
                .find(|m| SYSTEM_MOUNTS.contains(&m.as_str()));
            match critical {
                Some(mp) if system.is_none() => {
                    system = Some(format!("{pname} trägt über {holder} das Dateisystem {mp}"));
                }
                _ if busy.is_none() => {
                    busy = Some(format!(
                        "{pname} wird von {holder} verwendet (LVM/RAID/Verschlüsselung)"
                    ))
                }
                _ => {}
            }
        }
    }

    let device = dir.join("device");
    RawDrive {
        id: format!("/dev/{name}"),
        // Bevorzugt die Namen aus dem USB-Gerät („SanDisk“ / „Ultra“), sonst SCSI-Angaben.
        vendor: usb_dev
            .as_ref()
            .and_then(|u| read(u.join("manufacturer")))
            .or_else(|| read_name(device.join("vendor"))),
        model: usb_dev
            .as_ref()
            .and_then(|u| read(u.join("product")))
            .or_else(|| read_name(device.join("model"))),
        serial: usb_dev
            .as_ref()
            .and_then(|u| read(u.join("serial")))
            .or_else(|| read(dir.join("serial"))),
        size: read(dir.join("size"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
            * 512,
        bus,
        removable: read_flag(dir.join("removable")),
        read_only: read_flag(dir.join("ro")),
        system,
        busy,
        mounts: mountpoints,
        // Doppelte Absicherung: USB-Massenspeicher heißen unter Linux immer sdX.
        platform_ok: is_sd_name(name),
    }
}

fn is_sd_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with("sd") && name[2..].chars().all(|c| c.is_ascii_lowercase())
}

fn bus_from_path(path: &str, name: &str) -> Bus {
    if path.contains("/virtual/") {
        Bus::Virtual
    } else if path.contains("/usb") {
        Bus::Usb
    } else if name.starts_with("nvme") || path.contains("/nvme") {
        Bus::Nvme
    } else if name.starts_with("mmcblk") || path.contains("/mmc") {
        Bus::Mmc
    } else if name.starts_with("vd") || path.contains("/virtio") {
        Bus::Virtio
    } else if path.contains("/ata") {
        Bus::Sata
    } else if name.starts_with("sd") {
        Bus::Scsi
    } else {
        Bus::Other
    }
}

/// Geht im Gerätebaum nach oben bis zum USB-Gerät (dem Ordner mit `idVendor`).
fn find_usb_device(real: &Path) -> Option<PathBuf> {
    real.ancestors()
        .find(|p| p.join("idVendor").exists())
        .map(Path::to_owned)
}

fn list_names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// `/proc/self/mountinfo` → „Major:Minor“ → Einhängepunkte.
///
/// Format (man 5 proc): `36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw`
/// Feld 3 ist Major:Minor, Feld 5 der Einhängepunkt. Wir gleichen über die Gerätenummer ab,
/// nicht über den Pfad in Spalte „source“ – der kann `/dev/disk/by-uuid/…` oder sonstwas sein.
pub fn parse_mountinfo(text: &str) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() >= 5 {
            map.entry(f[2].to_owned()).or_default().push(unescape(f[4]));
        }
    }
    map
}

/// Leerzeichen & Co. stehen in mountinfo als Oktal-Escape (`\040`).
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let oct: String = chars.by_ref().take(3).collect();
            match u8::from_str_radix(&oct, 8) {
                Ok(b) => out.push(b as char),
                Err(_) => {
                    out.push('\\');
                    out.push_str(&oct);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `/proc/swaps` → Gerätenamen (ohne `/dev/`).
pub fn parse_swaps(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|p| p.strip_prefix("/dev/"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_by_device_number_and_escapes() {
        let m = parse_mountinfo(
            "23 1 8:2 / / rw - ext4 /dev/sda2 rw\n\
             40 23 8:17 / /media/user/MEIN\\040STICK rw - vfat /dev/sdb1 rw\n",
        );
        assert_eq!(m["8:2"], ["/"]);
        assert_eq!(m["8:17"], ["/media/user/MEIN STICK"]);
    }

    #[test]
    fn swaps() {
        let s = parse_swaps(
            "Filename Type Size Used Priority\n/dev/sda3 partition 1 0 -2\n/swapfile file 1 0 -3\n",
        );
        assert_eq!(s, ["sda3"]);
    }

    #[test]
    fn sd_names() {
        assert!(is_sd_name("sdb"));
        assert!(is_sd_name("sdaa"));
        assert!(!is_sd_name("sd"));
        assert!(!is_sd_name("sdb1"));
        assert!(!is_sd_name("nvme0n1"));
    }
}
