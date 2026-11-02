//! Ein FAT32-Dateisystem „in einem Rutsch“ erzeugen.
//!
//! Warum selbst geschrieben? Die Bibliothek `fatfs` (0.3.x) legt vor den Pflichteinträgen `.`
//! und `..` zusätzlich Langnamen-Einträge an. Das verstößt gegen die FAT-Spezifikation –
//! `fsck.vfat` meldet es als Fehler (in den Tests gefunden). Für einen Installationsstick, von dem
//! eine UEFI-Firmware starten soll, ist das zu riskant.
//!
//! Unser Fall ist einfach: Alle Dateien und Verzeichnisse stehen vorher fest, nichts wird
//! geändert oder gelöscht. Deshalb wird alles **vorab geplant**:
//!
//! ```text
//! Partition:  [Reserviert 32 Sektoren: Bootsektor, FSInfo, Kopien] [FAT 1] [FAT 2] [Datenbereich …]
//! Datenbereich: Cluster 2 = Wurzelverzeichnis, danach alle Verzeichnisse, dann jede Datei
//!               am Stück (zusammenhängend → große, schnelle Schreibzugriffe)
//! ```
//!
//! Der Plan liefert alle Metadaten (Bootsektor, FAT, Verzeichnisse) als Bytes und je Datei den
//! Byte-Bereich. So kann nach dem Schreiben **alles** zurückgelesen und verglichen werden.

use std::collections::{BTreeMap, HashSet};
use std::io;

const SECTOR: u64 = 512;
const RESERVED_SECTORS: u64 = 32;
const ATTR_DIR: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_VOLUME: u8 = 0x08;
const ATTR_LFN: u8 = 0x0F;
/// FAT32 braucht mindestens so viele Cluster (sonst wäre es per Definition FAT16).
const MIN_CLUSTERS: u64 = 65_525;

/// Ein Stück Metadaten: Position in der Partition + Inhalt.
pub struct Region {
    pub offset: u64,
    pub data: Vec<u8>,
}

/// Wo eine Datei landet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePlacement {
    pub dest: String,
    pub offset: u64,
    pub size: u64,
}

pub struct Plan {
    pub bytes_per_cluster: u64,
    pub total_clusters: u64,
    pub free_clusters: u64,
    /// Bootsektor, FSInfo, Kopien, beide FATs, alle Verzeichnisse.
    pub metadata: Vec<Region>,
    pub files: Vec<FilePlacement>,
}

/// Clustergröße nach Microsofts FAT32-Vorgaben.
fn preferred_cluster(len: u64) -> u64 {
    const GIB: u64 = 1024 * 1024 * 1024;
    match len {
        n if n >= 32 * GIB => 32 * 1024,
        n if n >= 16 * GIB => 16 * 1024,
        n if n >= 8 * GIB => 8 * 1024,
        n if n >= 260 * 1024 * 1024 => 4 * 1024,
        _ => 512,
    }
}

/// (Sektoren pro FAT, Anzahl Cluster) – iterativ, weil die FAT-Größe von der Clusterzahl abhängt.
fn geometry(total_sectors: u64, spc: u64) -> (u64, u64) {
    let mut fat_sectors = 1;
    loop {
        let data_sectors = total_sectors - RESERVED_SECTORS - 2 * fat_sectors;
        let clusters = data_sectors / spc;
        let needed = ((clusters + 2) * 4).div_ceil(SECTOR);
        if needed <= fat_sectors {
            return (fat_sectors, clusters);
        }
        fat_sectors = needed;
    }
}

// --- Namen ----------------------------------------------------------------------------------

const SFN_INVALID: &[char] = &[
    '"', '*', '+', ',', '.', '/', ':', ';', '<', '=', '>', '?', '[', '\\', ']', '|', ' ',
];

fn sfn_part(s: &str, max: usize) -> (String, bool) {
    let mut lossy = false;
    let mut out = String::new();
    for c in s.chars() {
        if out.len() == max {
            lossy = true;
            break;
        }
        if !c.is_ascii() || SFN_INVALID.contains(&c) {
            lossy = true;
            if c != ' ' && c != '.' {
                out.push('_');
            }
        } else {
            out.push(c.to_ascii_uppercase());
        }
    }
    (out, lossy)
}

/// 8.3-Kurzname (11 Bytes) und ob zusätzlich ein Langname nötig ist.
fn short_name(name: &str, used: &mut HashSet<[u8; 11]>) -> ([u8; 11], bool) {
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    };
    let (b, lossy_b) = sfn_part(base, 8);
    let (e, lossy_e) = sfn_part(ext, 3);
    // Gültiger 8.3-Name (höchstens Kleinbuchstaben)? Dann exakter Kurzname in Großbuchstaben –
    // wichtig für `EFI/BOOT/BOOTX64.EFI`, das manche UEFI-Firmware über den Kurznamen sucht.
    let exact = !lossy_b && !lossy_e && !b.is_empty();
    let has_lower = name.chars().any(|c| c.is_ascii_lowercase());
    let pack = |b: &str, e: &str| {
        let mut n = [b' '; 11];
        n[..b.len()].copy_from_slice(b.as_bytes());
        n[8..8 + e.len()].copy_from_slice(e.as_bytes());
        n
    };
    if exact {
        let n = pack(&b, &e);
        if used.insert(n) {
            return (n, has_lower);
        }
    }
    // „NAME~1.EXT“, „NAME~2.EXT“ … bis frei.
    let b = if b.is_empty() { "_".to_owned() } else { b };
    for i in 1..1_000_000u32 {
        let tail = format!("~{i}");
        let keep = 8 - tail.len();
        let n = pack(&format!("{}{tail}", &b[..b.len().min(keep)]), &e);
        if used.insert(n) {
            return (n, true);
        }
    }
    unreachable!("eine Million gleiche Kurznamen")
}

fn lfn_checksum(sfn: &[u8; 11]) -> u8 {
    sfn.iter()
        .fold(0u8, |sum, &b| sum.rotate_right(1).wrapping_add(b))
}

/// Langname-Einträge (in Schreib-Reihenfolge: letzter Teil zuerst).
fn lfn_entries(name: &str, sfn: &[u8; 11]) -> Vec<[u8; 32]> {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    if !units.len().is_multiple_of(13) {
        units.push(0);
        while !units.len().is_multiple_of(13) {
            units.push(0xFFFF);
        }
    }
    let count = units.len() / 13;
    let sum = lfn_checksum(sfn);
    (0..count)
        .rev()
        .map(|i| {
            let mut e = [0u8; 32];
            e[0] = (i as u8 + 1) | if i == count - 1 { 0x40 } else { 0 };
            e[11] = ATTR_LFN;
            e[13] = sum;
            let chunk = &units[i * 13..i * 13 + 13];
            let slots = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
            for (u, at) in chunk.iter().zip(slots) {
                e[at..at + 2].copy_from_slice(&u.to_le_bytes());
            }
            e
        })
        .collect()
}

/// Feste Zeitstempel (1.1.2026 12:00) – reproduzierbar und für Installationsmedien egal.
const DOS_DATE: u16 = ((2026 - 1980) << 9) | (1 << 5) | 1;
const DOS_TIME: u16 = 12 << 11;

fn sfn_entry(name: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
    let mut e = [0u8; 32];
    e[0..11].copy_from_slice(name);
    e[11] = attr;
    for at in [14, 22] {
        e[at..at + 2].copy_from_slice(&DOS_TIME.to_le_bytes());
    }
    for at in [16, 18, 24] {
        e[at..at + 2].copy_from_slice(&DOS_DATE.to_le_bytes());
    }
    e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
    e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
    e[28..32].copy_from_slice(&size.to_le_bytes());
    e
}

// --- Plan -----------------------------------------------------------------------------------

#[derive(Default)]
struct DirNode {
    subdirs: BTreeMap<String, DirNode>,
    files: Vec<(String, usize)>, // (Name, Index in `files`)
    cluster: u32,
    clusters: u64,
}

/// Plant das Dateisystem. `dirs`/`files` mit `/` getrennt, relativ zur Wurzel.
pub fn plan(
    partition_len: u64,
    partition_lba: u64,
    label: &str,
    dirs: &[String],
    files: &[(String, u64)],
) -> io::Result<Plan> {
    let total_sectors = partition_len / SECTOR;
    // Clustergröße notfalls verkleinern, damit es genug Cluster für FAT32 gibt.
    let mut bpc = preferred_cluster(partition_len);
    let (fat_sectors, total_clusters) = loop {
        let (fs, tc) = geometry(total_sectors, bpc / SECTOR);
        if tc >= MIN_CLUSTERS || bpc == SECTOR {
            break (fs, tc);
        }
        bpc /= 2;
    };
    if total_clusters < MIN_CLUSTERS {
        return Err(io::Error::other(
            "Partition zu klein für FAT32 (mind. ~33 MB)",
        ));
    }
    let data_start = (RESERVED_SECTORS + 2 * fat_sectors) * SECTOR;

    // Baum aufbauen
    let mut root = DirNode::default();
    fn node<'a>(root: &'a mut DirNode, path: &str) -> &'a mut DirNode {
        path.split('/')
            .filter(|p| !p.is_empty())
            .fold(root, |n, p| n.subdirs.entry(p.to_owned()).or_default())
    }
    for d in dirs {
        node(&mut root, d);
    }
    for (i, (dest, _)) in files.iter().enumerate() {
        let (parent, name) = dest.rsplit_once('/').unwrap_or(("", dest));
        node(&mut root, parent).files.push((name.to_owned(), i));
    }

    // Größe jedes Verzeichnisses (Einträge × 32 Bytes) → Cluster vergeben.
    fn entry_count(n: &DirNode, is_root: bool) -> u64 {
        let names = n.subdirs.keys().chain(n.files.iter().map(|(f, _)| f));
        let per: u64 = names
            .map(|name| 1 + (name.encode_utf16().count() as u64).div_ceil(13))
            .sum();
        per + if is_root { 1 } else { 2 } // Volume-Label bzw. „.“ und „..“
    }
    let mut next: u64 = 2;
    fn assign_dirs(n: &mut DirNode, is_root: bool, next: &mut u64, bpc: u64) {
        n.clusters = (entry_count(n, is_root) * 32).div_ceil(bpc).max(1);
        n.cluster = *next as u32;
        *next += n.clusters;
        for c in n.subdirs.values_mut() {
            assign_dirs(c, false, next, bpc);
        }
    }
    assign_dirs(&mut root, true, &mut next, bpc);

    // Dateien am Stück dahinter
    let mut file_cluster = vec![0u32; files.len()];
    let mut placements = Vec::with_capacity(files.len());
    for (i, (dest, size)) in files.iter().enumerate() {
        let n = size.div_ceil(bpc);
        if n > 0 {
            file_cluster[i] = next as u32;
        }
        placements.push(FilePlacement {
            dest: dest.clone(),
            offset: data_start + (next.max(2) - 2) * bpc,
            size: *size,
        });
        next += n;
    }
    if next - 2 > total_clusters {
        return Err(io::Error::other("Dateien passen nicht auf den Stick"));
    }
    let used = next - 2;

    // FAT: Ketten für alle Verzeichnisse und Dateien (zusammenhängend → n → n+1 → … → Ende).
    let mut fat = vec![0u32; (total_clusters + 2) as usize];
    fat[0] = 0x0FFF_FFF8;
    fat[1] = 0x0FFF_FFFF;
    let mut chain = |first: u64, count: u64| {
        for c in first..first + count {
            fat[c as usize] = if c + 1 == first + count {
                0x0FFF_FFFF
            } else {
                (c + 1) as u32
            };
        }
    };
    fn dir_chains(n: &DirNode, chain: &mut dyn FnMut(u64, u64)) {
        chain(n.cluster as u64, n.clusters);
        for c in n.subdirs.values() {
            dir_chains(c, chain);
        }
    }
    dir_chains(&root, &mut chain);
    for (i, (_, size)) in files.iter().enumerate() {
        if file_cluster[i] != 0 {
            chain(file_cluster[i] as u64, size.div_ceil(bpc));
        }
    }
    let mut fat_bytes = vec![0u8; (fat_sectors * SECTOR) as usize];
    for (i, v) in fat.iter().enumerate() {
        fat_bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }

    // Verzeichnisinhalte
    let mut metadata = Vec::new();
    let label11 = {
        let mut l = [b' '; 11];
        for (i, b) in label
            .bytes()
            .filter(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .take(11)
            .enumerate()
        {
            l[i] = b.to_ascii_uppercase();
        }
        l
    };
    /// Was alle Verzeichnisse gemeinsam brauchen – statt neun einzelner Parameter.
    struct Emitter<'a> {
        label: [u8; 11],
        files: &'a [(String, u64)],
        file_cluster: &'a [u32],
        bpc: u64,
        data_start: u64,
        out: Vec<Region>,
    }
    impl Emitter<'_> {
        fn dir(&mut self, n: &DirNode, parent_cluster: u32, is_root: bool) -> io::Result<()> {
            let mut buf = Vec::new();
            if is_root {
                buf.extend_from_slice(&sfn_entry(&self.label, ATTR_VOLUME, 0, 0));
            } else {
                let mut dot = [b' '; 11];
                dot[0] = b'.';
                buf.extend_from_slice(&sfn_entry(&dot, ATTR_DIR, n.cluster, 0));
                dot[1] = b'.';
                // „..“ zeigt auf das Elternverzeichnis – bei der Wurzel per Definition auf 0.
                buf.extend_from_slice(&sfn_entry(&dot, ATTR_DIR, parent_cluster, 0));
            }
            let mut used = HashSet::new();
            let mut add = |name: &str, attr: u8, cluster: u32, size: u64| -> io::Result<()> {
                let size = u32::try_from(size)
                    .map_err(|_| io::Error::other(format!("{name}: größer als 4 GB")))?;
                let (sfn, needs_lfn) = short_name(name, &mut used);
                if needs_lfn {
                    for e in lfn_entries(name, &sfn) {
                        buf.extend_from_slice(&e);
                    }
                }
                buf.extend_from_slice(&sfn_entry(&sfn, attr, cluster, size));
                Ok(())
            };
            for (name, child) in &n.subdirs {
                add(name, ATTR_DIR, child.cluster, 0)?;
            }
            for (name, i) in &n.files {
                add(name, ATTR_ARCHIVE, self.file_cluster[*i], self.files[*i].1)?;
            }
            buf.resize((n.clusters * self.bpc) as usize, 0);
            self.out.push(Region {
                offset: self.data_start + (n.cluster as u64 - 2) * self.bpc,
                data: buf,
            });
            let me = if is_root { 0 } else { n.cluster };
            for child in n.subdirs.values() {
                self.dir(child, me, false)?;
            }
            Ok(())
        }
    }
    let mut emitter = Emitter {
        label: label11,
        files,
        file_cluster: &file_cluster,
        bpc,
        data_start,
        out: Vec::new(),
    };
    emitter.dir(&root, 0, true)?;
    metadata.extend(emitter.out);

    // Bootsektor (BPB) + FSInfo
    let mut boot = vec![0u8; SECTOR as usize];
    boot[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    boot[3..11].copy_from_slice(b"MSWIN4.1");
    boot[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    boot[13] = (bpc / SECTOR) as u8;
    boot[14..16].copy_from_slice(&(RESERVED_SECTORS as u16).to_le_bytes());
    boot[16] = 2; // zwei FATs
    boot[21] = 0xF8; // Festplatte
    boot[24..26].copy_from_slice(&63u16.to_le_bytes());
    boot[26..28].copy_from_slice(&255u16.to_le_bytes());
    boot[28..32].copy_from_slice(&(partition_lba as u32).to_le_bytes());
    boot[32..36].copy_from_slice(&(total_sectors as u32).to_le_bytes());
    boot[36..40].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    boot[44..48].copy_from_slice(&2u32.to_le_bytes()); // Wurzel = Cluster 2
    boot[48..50].copy_from_slice(&1u16.to_le_bytes()); // FSInfo in Sektor 1
    boot[50..52].copy_from_slice(&6u16.to_le_bytes()); // Kopie in Sektor 6
    boot[64] = 0x80;
    boot[66] = 0x29;
    let vol_id = (crc32fast::hash(&label11) ^ (total_sectors as u32)).to_le_bytes();
    boot[67..71].copy_from_slice(&vol_id);
    boot[71..82].copy_from_slice(&label11);
    boot[82..90].copy_from_slice(b"FAT32   ");
    boot[510] = 0x55;
    boot[511] = 0xAA;

    let free = total_clusters - used;
    let mut fsinfo = vec![0u8; SECTOR as usize];
    fsinfo[0..4].copy_from_slice(&0x4161_5252u32.to_le_bytes());
    fsinfo[484..488].copy_from_slice(&0x6141_7272u32.to_le_bytes());
    fsinfo[488..492].copy_from_slice(&(free as u32).to_le_bytes());
    fsinfo[492..496].copy_from_slice(&(next as u32).to_le_bytes());
    fsinfo[508..512].copy_from_slice(&0xAA55_0000u32.to_le_bytes());

    // Reservierter Bereich komplett (alte Reste genullt), mit Kopien in Sektor 6/7.
    let mut reserved = vec![0u8; (RESERVED_SECTORS * SECTOR) as usize];
    for base in [0usize, 6 * SECTOR as usize] {
        reserved[base..base + 512].copy_from_slice(&boot);
        reserved[base + 512..base + 1024].copy_from_slice(&fsinfo);
    }
    metadata.insert(
        0,
        Region {
            offset: 0,
            data: reserved,
        },
    );
    metadata.insert(
        1,
        Region {
            offset: RESERVED_SECTORS * SECTOR,
            data: fat_bytes.clone(),
        },
    );
    metadata.insert(
        2,
        Region {
            offset: (RESERVED_SECTORS + fat_sectors) * SECTOR,
            data: fat_bytes,
        },
    );

    Ok(Plan {
        bytes_per_cluster: bpc,
        total_clusters,
        free_clusters: free,
        metadata,
        files: placements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names() {
        let mut u = HashSet::new();
        assert_eq!(short_name("SETUP.EXE", &mut u), (*b"SETUP   EXE", false));
        // Kleinbuchstaben → Langname nötig, Kurzname bleibt lesbar
        assert_eq!(short_name("bootmgr", &mut u), (*b"BOOTMGR    ", true));
        assert_eq!(short_name("bootx64.efi", &mut u), (*b"BOOTX64 EFI", true));
        assert_eq!(
            short_name("BOOTMGR", &mut u),
            (*b"BOOTMG~1   ", true),
            "Kollision mit „bootmgr“"
        );
        // Kollision
        assert_eq!(
            short_name("Datei mit langem Namen.txt", &mut u).0,
            *b"DATEIM~1TXT"
        );
        assert_eq!(
            short_name("Datei mit langem Namen 2.txt", &mut u).0,
            *b"DATEIM~2TXT"
        );
    }

    #[test]
    fn lfn_checksum_known_value() {
        // Prüfsumme aus Microsofts FAT-Spezifikation nachgerechnet
        let mut s: u8 = 0;
        for b in *b"SETUP   EXE" {
            s = ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(b);
        }
        assert_eq!(lfn_checksum(b"SETUP   EXE"), s);
    }

    #[test]
    fn geometry_is_consistent() {
        let sectors = 8_000_000_000u64 / 512;
        let (fat, clusters) = geometry(sectors, 8);
        assert!((clusters + 2) * 4 <= fat * 512);
        assert!(RESERVED_SECTORS + 2 * fat + clusters * 8 <= sectors);
    }
}
