//! Schreiben + Zurücklesen mit Dateien als „Stick“ – inklusive defekter und gefälschter Sticks.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use bootforge_core::catalog::{Compression, HashAlgorithm};
use bootforge_core::flash::write::{ALIGN, Device, verify_image, write_and_verify};
use bootforge_core::flash::{FlashError, FlashEvent, FlashJob, FlashStage};
use bootforge_core::verify::sums::Hasher;

const MB: u64 = 1024 * 1024;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bf-flash-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Pseudozufällige Daten (nicht nur Nullen, sonst fallen Fehler nicht auf).
fn data(len: usize) -> Vec<u8> {
    let mut x: u32 = 2463534242;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn hash(alg: HashAlgorithm, d: &[u8]) -> String {
    let mut h = Hasher::new(alg);
    h.update(d);
    h.finish_hex()
}

fn image(dir: &Path, name: &str, content: &[u8], compression: Compression) -> FlashJob {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    FlashJob {
        image: path,
        compression,
        algorithm: HashAlgorithm::Sha256,
        expected_hash: hash(HashAlgorithm::Sha256, content),
    }
}

/// Leerer „Stick“ der Größe `size`, vorher mit 0xAA gefüllt (um Reste zu erkennen).
fn stick(dir: &Path, size: u64) -> File {
    let p = dir.join("stick.bin");
    std::fs::write(&p, vec![0xAA; size as usize]).unwrap();
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p)
        .unwrap()
}

fn read_all(f: &mut File) -> Vec<u8> {
    let mut v = Vec::new();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.read_to_end(&mut v).unwrap();
    v
}

fn run(
    dev: &mut impl Device,
    size: u64,
    job: &FlashJob,
) -> (
    Result<bootforge_core::flash::FlashReport, FlashError>,
    Vec<FlashStage>,
) {
    let mut stages = Vec::new();
    let no = AtomicBool::new(false);
    let mut on = |e: FlashEvent| {
        if let FlashEvent::Stage { stage } = e {
            stages.push(stage)
        }
    };
    let r = verify_image(job, &no, &mut on)
        .and_then(|_| write_and_verify(dev, size, job, &no, &mut on));
    (r, stages)
}

#[test]
fn raw_image_written_padded_and_verified() {
    let d = dir("raw");
    // Absichtlich krumme Größe: 5 MB + 123 Bytes → letzter Block wird aufgefüllt.
    let content = data(5 * MB as usize + 123);
    let job = image(&d, "x.iso", &content, Compression::None);
    let mut dev = stick(&d, 16 * MB);

    let (r, stages) = run(&mut dev, 16 * MB, &job);
    let report = r.unwrap();
    assert_eq!(
        stages,
        [
            FlashStage::VerifyImage,
            FlashStage::Write,
            FlashStage::Readback
        ]
    );
    assert_eq!(report.written, content.len() as u64);
    assert_eq!(report.sha256, hash(HashAlgorithm::Sha256, &content));

    let disk = read_all(&mut dev);
    assert_eq!(&disk[..content.len()], &content[..]);
    let padded_end = content.len().div_ceil(ALIGN) * ALIGN;
    assert!(
        disk[content.len()..padded_end].iter().all(|&b| b == 0),
        "Auffüllung mit Nullen"
    );
    assert!(
        disk[padded_end..].iter().all(|&b| b == 0xAA),
        "dahinter unberührt"
    );
}

#[test]
fn gzip_image_is_decompressed_on_the_fly() {
    let d = dir("gzip");
    let content = data(3 * MB as usize + 7);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(&content).unwrap();
    let compressed = gz.finish().unwrap();
    // Wie bei Batocera: die Prüfsumme gilt für die .gz-Datei, nicht für den Inhalt.
    let mut job = image(&d, "x.img.gz", &compressed, Compression::Gzip);
    job.algorithm = HashAlgorithm::Md5;
    job.expected_hash = hash(HashAlgorithm::Md5, &compressed);

    let mut dev = stick(&d, 8 * MB);
    let report = run(&mut dev, 8 * MB, &job).0.unwrap();
    assert_eq!(report.written, content.len() as u64);
    assert_eq!(&read_all(&mut dev)[..content.len()], &content[..]);
}

/// Raspberry-Pi-Images kommen als `.img.xz`, manche als `.img.zst` – ebenfalls im Fluss entpackt.
#[test]
fn xz_and_zstd_images_are_decompressed_on_the_fly() {
    let content = data(2 * MB as usize + 13);

    let mut xz = xz2::write::XzEncoder::new(Vec::new(), 1);
    xz.write_all(&content).unwrap();
    let xz = xz.finish().unwrap();
    let zst = ruzstd::encoding::compress_to_vec(
        content.as_slice(),
        ruzstd::encoding::CompressionLevel::Fastest,
    );

    for (name, compressed, c) in [
        ("x.img.xz", xz, Compression::Xz),
        ("x.img.zst", zst, Compression::Zstd),
    ] {
        let d = dir(name);
        let mut job = image(&d, name, &compressed, c);
        job.expected_hash = hash(HashAlgorithm::Sha256, &compressed);
        let mut dev = stick(&d, 8 * MB);
        let report = run(&mut dev, 8 * MB, &job).0.unwrap();
        assert_eq!(report.written, content.len() as u64, "{name}");
        assert_eq!(&read_all(&mut dev)[..content.len()], &content[..], "{name}");
    }
}

#[test]
fn wrong_image_never_touches_the_stick() {
    let d = dir("wrongimg");
    let mut job = image(&d, "x.iso", &data(MB as usize), Compression::None);
    job.expected_hash = "0".repeat(64);
    let mut dev = stick(&d, 4 * MB);
    let (r, stages) = run(&mut dev, 4 * MB, &job);
    assert!(matches!(r, Err(FlashError::ImageMismatch { .. })), "{r:?}");
    assert_eq!(
        stages,
        [FlashStage::VerifyImage],
        "Schreiben darf nicht beginnen"
    );
    assert!(
        read_all(&mut dev).iter().all(|&b| b == 0xAA),
        "Stick unverändert"
    );
}

#[test]
fn image_swapped_after_check_is_detected_and_stick_wiped() {
    let d = dir("swap");
    let job = image(&d, "x.iso", &data(2 * MB as usize), Compression::None);
    let mut dev = stick(&d, 4 * MB);
    let no = AtomicBool::new(false);
    verify_image(&job, &no, &mut |_| {}).unwrap();
    // Angreifer tauscht die Datei NACH der Prüfung aus:
    std::fs::write(&job.image, data(2 * MB as usize + 1)).unwrap();
    let r = write_and_verify(&mut dev, 4 * MB, &job, &no, &mut |_| {});
    assert_eq!(r, Err(FlashError::ImageChanged));
    assert!(
        read_all(&mut dev)[..MB as usize].iter().all(|&b| b == 0),
        "Anfang gelöscht"
    );
}

#[test]
fn too_small_stick_rejected_before_writing() {
    let d = dir("small");
    let job = image(&d, "x.iso", &data(3 * MB as usize), Compression::None);
    let mut dev = stick(&d, 2 * MB);
    let (r, _) = run(&mut dev, 2 * MB, &job);
    assert!(matches!(r, Err(FlashError::TooSmall { .. })), "{r:?}");
    assert!(read_all(&mut dev).iter().all(|&b| b == 0xAA));
}

#[test]
fn cancel_wipes_head() {
    let d = dir("cancel");
    let job = image(&d, "x.iso", &data(10 * MB as usize), Compression::None);
    let mut dev = stick(&d, 16 * MB);
    let cancel = AtomicBool::new(false);
    verify_image(&job, &cancel, &mut |_| {}).unwrap();
    cancel.store(true, Ordering::Relaxed);
    let r = write_and_verify(&mut dev, 16 * MB, &job, &cancel, &mut |_| {});
    assert_eq!(r, Err(FlashError::Cancelled));
    assert!(read_all(&mut dev)[..MB as usize].iter().all(|&b| b == 0));
}

// --- defekte Sticks --------------------------------------------------------------------------

/// Speichert an einer Stelle ein falsches Bit (kaputter Flash-Speicher).
struct FlakyStick {
    inner: File,
    bad_offset: u64,
}

impl Read for FlakyStick {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}
impl Seek for FlakyStick {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}
impl Write for FlakyStick {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let pos = self.inner.stream_position()?;
        let mut copy = buf.to_vec();
        if (pos..pos + buf.len() as u64).contains(&self.bad_offset) {
            copy[(self.bad_offset - pos) as usize] ^= 0x01;
        }
        self.inner.write(&copy)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl Device for FlakyStick {
    fn sync(&mut self) -> io::Result<()> {
        self.inner.sync()
    }
}

#[test]
fn defective_stick_detected_with_offset() {
    let d = dir("flaky");
    let job = image(&d, "x.iso", &data(6 * MB as usize), Compression::None);
    let mut dev = FlakyStick {
        inner: stick(&d, 8 * MB),
        bad_offset: 5 * MB + 4321,
    };
    let (r, _) = run(&mut dev, 8 * MB, &job);
    assert_eq!(
        r,
        Err(FlashError::ReadbackMismatch {
            offset: 5 * MB + 4321
        })
    );
}

/// Gefälschter Stick: gibt 8 MB an, speichert aber nur 4 MB und wiederholt sie dann
/// (typisch für Billig-Fälschungen, die „64 GB“ versprechen).
struct FakeCapacity {
    inner: File,
    real: u64,
    pos: u64,
}

impl Read for FakeCapacity {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.seek(SeekFrom::Start(self.pos % self.real))?;
        let len = buf.len().min((self.real - self.pos % self.real) as usize);
        let n = self.inner.read(&mut buf[..len])?;
        self.pos += n as u64;
        Ok(n)
    }
}
impl Seek for FakeCapacity {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        if let SeekFrom::Start(p) = pos {
            self.pos = p;
        }
        Ok(self.pos)
    }
}
impl Write for FakeCapacity {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.seek(SeekFrom::Start(self.pos % self.real))?;
        let n = self
            .inner
            .write(&buf[..buf.len().min((self.real - self.pos % self.real) as usize)])?;
        self.pos += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl Device for FakeCapacity {
    fn sync(&mut self) -> io::Result<()> {
        self.inner.sync()
    }
}

#[test]
fn fake_capacity_stick_detected() {
    let d = dir("fake");
    let job = image(&d, "x.iso", &data(6 * MB as usize), Compression::None);
    let mut dev = FakeCapacity {
        inner: stick(&d, 4 * MB),
        real: 4 * MB,
        pos: 0,
    };
    let (r, _) = run(&mut dev, 8 * MB, &job);
    // Die ersten 4 MB wurden von den letzten 2 MB überschrieben → Abweichung ab Byte 0.
    assert_eq!(r, Err(FlashError::ReadbackMismatch { offset: 0 }));
}
