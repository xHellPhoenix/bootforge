//! GPT-Partitionstabelle mit einer einzigen FAT32-Partition schreiben.
//!
//! Aufbau (Sektoren zu 512 Bytes, UEFI-Spezifikation Kap. 5):
//!
//! ```text
//! LBA 0          Protective MBR   – sagt alten Programmen „Datenträger ist belegt (GPT)“
//! LBA 1          GPT-Header       – Signatur „EFI PART“, CRC32, Verweise
//! LBA 2–33       Partitionseinträge (128 × 128 Bytes)
//! LBA 2048 …     unsere Partition (ab 1 MiB – ausgerichtet, gut für Flash-Speicher)
//! letzte 33 LBA  Kopie der Einträge + Kopie des Headers (Backup)
//! ```

use std::io::{self, Seek, SeekFrom, Write};

use super::blockcache::SECTOR;

const ENTRIES: usize = 128;
const ENTRY_SIZE: usize = 128;
const ENTRY_SECTORS: u64 = (ENTRIES * ENTRY_SIZE / SECTOR) as u64; // 32
const ALIGN_SECTORS: u64 = 2048; // 1 MiB

/// GUID-Typ „Microsoft Basic Data“ – damit erkennen Windows und UEFI-Firmware FAT32-Partitionen.
pub const BASIC_DATA: &str = "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7";

/// Wo die Partition liegt (in Bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionRange {
    pub start: u64,
    pub len: u64,
}

/// GUID-Text → 16 Bytes in GPT-Reihenfolge. Achtung: Die ersten drei Gruppen stehen
/// „little endian“ (rückwärts), die letzten zwei so, wie sie geschrieben sind.
pub fn guid_bytes(s: &str) -> [u8; 16] {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    assert_eq!(hex.len(), 32, "ungültige GUID {s}");
    let b: Vec<u8> = (0..16)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex"))
        .collect();
    [
        b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13],
        b[14], b[15],
    ]
}

/// Zufällige GUID (Version 4). Muss nur eindeutig sein, nicht geheim.
pub fn random_guid() -> [u8; 16] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut g = [0u8; 16];
    for (i, chunk) in g.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        chunk.copy_from_slice(&h.finish().to_le_bytes());
    }
    g[7] = (g[7] & 0x0f) | 0x40;
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

fn put_u32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// Berechnet Lage der Partition und alle Strukturen. `disk_bytes` muss ein Vielfaches von 512 sein.
pub struct Layout {
    pub partition: PartitionRange,
    mbr: Vec<u8>,
    primary_header: Vec<u8>,
    entries: Vec<u8>,
    backup_header: Vec<u8>,
    last_lba: u64,
}

pub fn layout(
    disk_bytes: u64,
    label: &str,
    disk_guid: [u8; 16],
    part_guid: [u8; 16],
) -> io::Result<Layout> {
    let sectors = disk_bytes / SECTOR as u64;
    if sectors < ALIGN_SECTORS * 4 {
        return Err(io::Error::other("Datenträger zu klein für GPT"));
    }
    let last_lba = sectors - 1;
    let first_usable = 2 + ENTRY_SECTORS; // 34
    let last_usable = last_lba - ENTRY_SECTORS - 1;
    let part_first = ALIGN_SECTORS;
    // Ende auf 1-MiB-Grenze abrunden (Flash-Speicher arbeitet in großen Blöcken).
    let part_last = (last_usable + 1) / ALIGN_SECTORS * ALIGN_SECTORS - 1;

    // --- Partitionseintrag ---
    let mut entries = vec![0u8; ENTRIES * ENTRY_SIZE];
    entries[0..16].copy_from_slice(&guid_bytes(BASIC_DATA));
    entries[16..32].copy_from_slice(&part_guid);
    put_u64(&mut entries, 32, part_first);
    put_u64(&mut entries, 40, part_last);
    // Attribute 0, Name in UTF-16LE (max. 36 Zeichen)
    for (i, unit) in label.encode_utf16().take(36).enumerate() {
        entries[56 + 2 * i..58 + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    let entries_crc = crc32fast::hash(&entries);

    // --- Header (primär und Backup unterscheiden sich in Lage und Verweisen) ---
    let header = |current: u64, backup: u64, entries_lba: u64| {
        let mut h = vec![0u8; SECTOR];
        h[0..8].copy_from_slice(b"EFI PART");
        put_u32(&mut h, 8, 0x0001_0000); // Revision 1.0
        put_u32(&mut h, 12, 92); // Header-Größe
        put_u64(&mut h, 24, current);
        put_u64(&mut h, 32, backup);
        put_u64(&mut h, 40, first_usable);
        put_u64(&mut h, 48, last_usable);
        h[56..72].copy_from_slice(&disk_guid);
        put_u64(&mut h, 72, entries_lba);
        put_u32(&mut h, 80, ENTRIES as u32);
        put_u32(&mut h, 84, ENTRY_SIZE as u32);
        put_u32(&mut h, 88, entries_crc);
        // Header-CRC über die 92 Bytes, mit CRC-Feld = 0 gerechnet.
        let crc = crc32fast::hash(&h[0..92]);
        put_u32(&mut h, 16, crc);
        h
    };
    let primary_header = header(1, last_lba, 2);
    let backup_header = header(last_lba, 1, last_lba - ENTRY_SECTORS);

    // --- Protective MBR ---
    let mut mbr = vec![0u8; SECTOR];
    let e = 446;
    mbr[e + 1..e + 4].copy_from_slice(&[0x00, 0x02, 0x00]); // CHS Start
    mbr[e + 4] = 0xEE; // Typ „GPT protective“
    mbr[e + 5..e + 8].copy_from_slice(&[0xFF, 0xFF, 0xFF]); // CHS Ende
    put_u32(&mut mbr, e + 8, 1);
    put_u32(&mut mbr, e + 12, last_lba.min(0xFFFF_FFFF) as u32);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    Ok(Layout {
        partition: PartitionRange {
            start: part_first * SECTOR as u64,
            len: (part_last - part_first + 1) * SECTOR as u64,
        },
        mbr,
        primary_header,
        entries,
        backup_header,
        last_lba,
    })
}

impl Layout {
    /// Schreibt alles. Vorher werden die ersten und letzten 1 MiB genullt: Dort stehen sonst
    /// Reste alter Partitionstabellen oder ISO-Kennungen („CD001“), die Betriebssysteme verwirren.
    pub fn write<W: Write + Seek>(&self, w: &mut W) -> io::Result<()> {
        let zeros = vec![0u8; (ALIGN_SECTORS as usize) * SECTOR];
        let disk_end = (self.last_lba + 1) * SECTOR as u64;
        w.seek(SeekFrom::Start(0))?;
        w.write_all(&zeros)?;
        w.seek(SeekFrom::Start(disk_end - zeros.len() as u64))?;
        w.write_all(&zeros)?;

        let at = |lba: u64| SeekFrom::Start(lba * SECTOR as u64);
        w.seek(at(0))?;
        w.write_all(&self.mbr)?;
        w.write_all(&self.primary_header)?;
        w.write_all(&self.entries)?;
        w.seek(at(self.last_lba - ENTRY_SECTORS))?;
        w.write_all(&self.entries)?;
        w.write_all(&self.backup_header)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_mixed_endian() {
        // Bekannte Bytefolge des Basic-Data-Typs, wie sie auf jedem Windows-Datenträger steht.
        assert_eq!(
            guid_bytes(BASIC_DATA),
            [
                0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26,
                0x99, 0xC7
            ]
        );
    }

    #[test]
    fn partition_aligned_and_inside_usable_area() {
        let size = 8_004_304_896u64; // typischer „8 GB“-Stick
        let l = layout(size, "BOOTFORGE", random_guid(), random_guid()).unwrap();
        assert_eq!(l.partition.start, 1024 * 1024);
        assert_eq!(l.partition.len % (1024 * 1024), 0);
        assert!(l.partition.start + l.partition.len <= size - 34 * 512);
    }

    #[test]
    fn header_crc_is_valid() {
        let l = layout(64 * 1024 * 1024, "X", random_guid(), random_guid()).unwrap();
        let mut h = l.primary_header[..92].to_vec();
        let stored = u32::from_le_bytes(h[16..20].try_into().unwrap());
        h[16..20].fill(0);
        assert_eq!(crc32fast::hash(&h), stored);
    }

    #[test]
    fn random_guids_differ_and_are_v4() {
        let (a, b) = (random_guid(), random_guid());
        assert_ne!(a, b);
        assert_eq!(a[7] >> 4, 4);
    }
}
