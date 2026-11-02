//! Image auf ein Gerät schreiben und durch Zurücklesen prüfen – plattformunabhängig.
//!
//! Das „Gerät“ ist hier nur etwas, das lesen, schreiben und springen kann ([`Device`]).
//! Im echten Betrieb ist es `/dev/sdb` bzw. `\\.\PhysicalDrive1`, in den Tests eine normale Datei.
//!
//! Dreifache Prüfung:
//! 1. [`verify_image`]: Hash der Image-Datei VOR dem Schreiben. Stimmt er nicht, wird der Stick
//!    gar nicht angefasst.
//! 2. Beim Schreiben wird die Datei erneut gehasht. Wurde sie zwischendurch ausgetauscht,
//!    wird der Anfang des Sticks gelöscht (damit er nicht mit Mischdaten bootet).
//! 3. Danach wird der Stick zurückgelesen und mit dem Geschriebenen verglichen
//!    (erkennt defekte oder gefälschte Sticks, die weniger speichern als sie vorgeben).
//!
//! Puffer sind auf 4096 Bytes ausgerichtet und Schreibzugriffe immer ein Vielfaches davon.
//! Das verlangen Windows (`FILE_FLAG_NO_BUFFERING`) und Linux (`O_DIRECT`) für direkten Zugriff
//! am Cache vorbei – nur so lesen wir beim Prüfen wirklich vom Stick und nicht aus dem RAM.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::catalog::{Compression, HashAlgorithm};
use crate::verify::sums::{Hasher, hashes_equal};

/// Ausrichtung für direkten Gerätezugriff (Vielfaches von 512 und 4096-Byte-Sektoren).
pub const ALIGN: usize = 4096;
/// Größe eines Schreibblocks.
pub const CHUNK: usize = 4 * 1024 * 1024;
/// So viel wird bei Abbruch/Fehler am Anfang genullt (Partitionstabellen + Bootsektoren).
const WIPE: usize = 1024 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Was geschrieben werden soll. Wird unter Linux als JSON an den Root-Helper übergeben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashJob {
    pub image: PathBuf,
    pub compression: Compression,
    pub algorithm: HashAlgorithm,
    /// Erwarteter Hash der Image-Datei (so wie heruntergeladen und geprüft).
    pub expected_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlashStage {
    /// Laufwerk unmittelbar vorher neu prüfen.
    CheckDrive,
    /// Image-Datei noch einmal hashen.
    VerifyImage,
    /// Volumes aushängen/sperren, Gerät öffnen.
    Prepare,
    Write,
    /// Zurücklesen und vergleichen.
    Readback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum FlashEvent {
    Stage {
        stage: FlashStage,
    },
    Progress {
        stage: FlashStage,
        done: u64,
        total: Option<u64>,
        bytes_per_sec: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlashReport {
    /// Geschriebene Nutzdaten in Bytes (ohne Auffüllung).
    pub written: u64,
    /// SHA-256 der geschriebenen (ggf. entpackten) Daten – identisch beim Zurücklesen.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub enum FlashError {
    #[error("Laufwerk hat sich geändert oder ist nicht mehr zulässig: {0}")]
    Drive(String),
    #[error(
        "Image-Datei stimmt nicht mit der geprüften Prüfsumme überein (erwartet {expected}, ist {actual})"
    )]
    ImageMismatch { expected: String, actual: String },
    #[error(
        "Image-Datei wurde während des Schreibens verändert – der Stick wurde unbrauchbar gemacht"
    )]
    ImageChanged,
    #[error("Stick zu klein: {need} Bytes nötig, {have} vorhanden")]
    TooSmall { need: u64, have: u64 },
    #[error(
        "Prüfung fehlgeschlagen: Die zurückgelesenen Daten weichen ab ab Byte {offset}. Der Stick ist vermutlich defekt."
    )]
    ReadbackMismatch { offset: u64 },
    #[error("Laufwerk wird noch verwendet: {0}")]
    Busy(String),
    #[error("Administratorrechte nötig")]
    NotElevated,
    #[error("nicht unterstützt: {0}")]
    Unsupported(String),
    /// Abgebrochen. Wurde schon geschrieben, ist der Anfang des Sticks gelöscht (nicht mehr bootbar).
    #[error("abgebrochen")]
    Cancelled,
    #[error("Ein-/Ausgabefehler: {0}")]
    Io(String),
}

impl From<io::Error> for FlashError {
    fn from(e: io::Error) -> Self {
        FlashError::Io(e.to_string())
    }
}

/// Ein beschreibbares Gerät. `sync` muss garantieren, dass alles physisch geschrieben ist.
pub trait Device: Read + Write + Seek {
    fn sync(&mut self) -> io::Result<()>;
}

impl Device for File {
    fn sync(&mut self) -> io::Result<()> {
        self.flush()?;
        self.sync_all()
    }
}

/// Auf [`ALIGN`] ausgerichteter Speicher. `Vec<u8>` garantiert keine Ausrichtung,
/// direkter Gerätezugriff verlangt sie aber – deshalb hier von Hand angelegt.
pub struct AlignedBuf {
    ptr: std::ptr::NonNull<u8>,
    len: usize,
}

// SAFETY: Der Puffer gehört exklusiv dieser Struktur (wie ein Vec<u8>).
unsafe impl Send for AlignedBuf {}

impl AlignedBuf {
    pub fn new(len: usize) -> Self {
        assert!(len > 0 && len.is_multiple_of(ALIGN));
        let layout = std::alloc::Layout::from_size_align(len, ALIGN).expect("gültiges Layout");
        // SAFETY: Layout hat Größe > 0; Rückgabe wird auf null geprüft.
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr =
            std::ptr::NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Self { ptr, len }
    }
}

impl std::ops::Deref for AlignedBuf {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        // SAFETY: ptr zeigt auf `len` initialisierte (genullte) Bytes, die uns gehören.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl std::ops::DerefMut for AlignedBuf {
    fn deref_mut(&mut self) -> &mut [u8] {
        // SAFETY: wie oben, und &mut self garantiert exklusiven Zugriff.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        let layout = std::alloc::Layout::from_size_align(self.len, ALIGN).expect("gültiges Layout");
        // SAFETY: mit genau diesem Layout angelegt.
        unsafe { std::alloc::dealloc(self.ptr.as_ptr(), layout) }
    }
}

fn round_up(n: u64) -> u64 {
    n.div_ceil(ALIGN as u64) * ALIGN as u64
}

/// Drosselt Fortschrittsmeldungen und misst die Geschwindigkeit.
struct Meter {
    last: Instant,
    last_done: u64,
    bps: f64,
}

impl Meter {
    fn new() -> Self {
        Self {
            last: Instant::now(),
            last_done: 0,
            bps: 0.0,
        }
    }
    fn tick(&mut self, done: u64) -> Option<f64> {
        let dt = self.last.elapsed();
        if dt < PROGRESS_INTERVAL {
            return None;
        }
        let sample = done.saturating_sub(self.last_done) as f64 / dt.as_secs_f64();
        self.bps = if self.bps == 0.0 {
            sample
        } else {
            0.3 * sample + 0.7 * self.bps
        };
        self.last = Instant::now();
        self.last_done = done;
        Some(self.bps)
    }
}

fn check(cancel: &AtomicBool) -> Result<(), FlashError> {
    if cancel.load(Ordering::Relaxed) {
        Err(FlashError::Cancelled)
    } else {
        Ok(())
    }
}

/// Schritt 1: Hash der Image-Datei prüfen. Liefert die Dateigröße.
pub fn verify_image(
    job: &FlashJob,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<u64, FlashError> {
    on_event(FlashEvent::Stage {
        stage: FlashStage::VerifyImage,
    });
    let mut f = File::open(&job.image)?;
    let total = f.metadata()?.len();
    let mut h = Hasher::new(job.algorithm);
    let mut buf = vec![0u8; CHUNK];
    let (mut done, mut meter) = (0u64, Meter::new());
    loop {
        check(cancel)?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        done += n as u64;
        if let Some(bps) = meter.tick(done) {
            on_event(FlashEvent::Progress {
                stage: FlashStage::VerifyImage,
                done,
                total: Some(total),
                bytes_per_sec: bps,
            });
        }
    }
    let actual = h.finish_hex();
    if !hashes_equal(&actual, &job.expected_hash) {
        return Err(FlashError::ImageMismatch {
            expected: job.expected_hash.clone(),
            actual,
        });
    }
    Ok(total)
}

/// Liest aus einer Datei, zählt mit und hasht nebenbei (für Schritt 2 der Prüfung).
struct CountingHasher<R> {
    inner: R,
    count: std::rc::Rc<std::cell::Cell<u64>>,
    hasher: std::rc::Rc<std::cell::RefCell<Option<Hasher>>>,
}

impl<R: Read> Read for CountingHasher<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.set(self.count.get() + n as u64);
        if let Some(h) = self.hasher.borrow_mut().as_mut() {
            h.update(&buf[..n]);
        }
        Ok(n)
    }
}

/// Füllt `buf` so weit wie möglich (ein einzelnes `read` liefert oft weniger).
fn fill(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Überschreibt den Anfang des Geräts mit Nullen, damit ein halb beschriebener Stick nicht
/// versehentlich bootet. Fehler werden ignoriert (das Gerät könnte schon abgezogen sein).
pub fn wipe_head<D: Device>(dev: &mut D) {
    let zeros = AlignedBuf::new(WIPE);
    let _ = dev
        .seek(SeekFrom::Start(0))
        .and_then(|_| dev.write_all(&zeros))
        .and_then(|_| dev.sync());
}

/// Schritte 2 und 3: schreiben (mit Hash) und zurücklesen.
///
/// Erwartet, dass [`verify_image`] vorher erfolgreich war.
pub fn write_and_verify<D: Device>(
    dev: &mut D,
    device_size: u64,
    job: &FlashJob,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(FlashEvent),
) -> Result<FlashReport, FlashError> {
    let file = File::open(&job.image)?;
    let file_size = file.metadata()?.len();
    if job.compression == Compression::None && round_up(file_size) > device_size {
        return Err(FlashError::TooSmall {
            need: round_up(file_size),
            have: device_size,
        });
    }

    let count = std::rc::Rc::new(std::cell::Cell::new(0u64));
    let source_hash = std::rc::Rc::new(std::cell::RefCell::new(Some(Hasher::new(job.algorithm))));
    let counting = CountingHasher {
        inner: BufReader::with_capacity(CHUNK, file),
        count: count.clone(),
        hasher: source_hash.clone(),
    };
    let mut reader: Box<dyn Read> = match job.compression {
        Compression::None => Box::new(counting),
        // MultiGz: manche .gz bestehen aus mehreren hintereinandergehängten Teilen.
        Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(counting)),
        other => return Err(FlashError::Unsupported(format!("Komprimierung {other:?}"))),
    };

    // --- schreiben ---
    on_event(FlashEvent::Stage {
        stage: FlashStage::Write,
    });
    dev.seek(SeekFrom::Start(0))?;
    let mut buf = AlignedBuf::new(CHUNK);
    let mut written_hash = Sha256::new();
    let (mut written, mut padded, mut meter) = (0u64, 0u64, Meter::new());

    let result: Result<(), FlashError> = (|| {
        loop {
            check(cancel)?;
            let n = fill(&mut *reader, &mut buf)?;
            if n == 0 {
                break;
            }
            // Letzter Block: mit Nullen auf die Ausrichtung auffüllen.
            let len = round_up(n as u64) as usize;
            buf[n..len].fill(0);
            if padded + len as u64 > device_size {
                return Err(FlashError::TooSmall {
                    need: padded + len as u64,
                    have: device_size,
                });
            }
            dev.write_all(&buf[..len])?;
            written_hash.update(&buf[..n]);
            written += n as u64;
            padded += len as u64;
            if let Some(bps) = meter.tick(count.get()) {
                on_event(FlashEvent::Progress {
                    stage: FlashStage::Write,
                    done: count.get(),
                    total: Some(file_size),
                    bytes_per_sec: bps,
                });
            }
            if n < CHUNK {
                // Rest der Quelle prüfen: ein zweiter, nicht voller Block wäre ein Fehler im Ablauf.
                let mut probe = [0u8; 1];
                if fill(&mut *reader, &mut probe)? != 0 {
                    return Err(FlashError::Io(
                        "unerwartete Daten nach dem letzten Block".into(),
                    ));
                }
                break;
            }
        }
        dev.sync()?;
        Ok(())
    })();
    drop(reader);

    if let Err(e) = result {
        wipe_head(dev);
        return Err(e);
    }

    // Schritt 2: Hat sich die Datei seit der Prüfung verändert?
    let actual = source_hash
        .borrow_mut()
        .take()
        .expect("einmalig")
        .finish_hex();
    if !hashes_equal(&actual, &job.expected_hash) {
        wipe_head(dev);
        return Err(FlashError::ImageChanged);
    }
    let sha256: String = written_hash
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    // --- Schritt 3: zurücklesen ---
    on_event(FlashEvent::Stage {
        stage: FlashStage::Readback,
    });
    dev.seek(SeekFrom::Start(0))?;
    let mut back = Sha256::new();
    let (mut pos, mut meter) = (0u64, Meter::new());
    while pos < padded {
        if cancel.load(Ordering::Relaxed) {
            // Daten sind komplett geschrieben; Abbruch der Prüfung macht den Stick nicht kaputt,
            // aber er gilt als ungeprüft.
            return Err(FlashError::Cancelled);
        }
        let len = (padded - pos).min(CHUNK as u64) as usize;
        dev.read_exact(&mut buf[..len])?;
        let useful = (written - pos).min(len as u64) as usize;
        back.update(&buf[..useful]);
        pos += len as u64;
        if let Some(bps) = meter.tick(pos) {
            on_event(FlashEvent::Progress {
                stage: FlashStage::Readback,
                done: pos,
                total: Some(padded),
                bytes_per_sec: bps,
            });
        }
    }
    let back_hex: String = back.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if back_hex != sha256 {
        let offset = find_first_difference(dev, job, written).unwrap_or(0);
        return Err(FlashError::ReadbackMismatch { offset });
    }
    Ok(FlashReport { written, sha256 })
}

/// Für eine hilfreiche Fehlermeldung: ab welchem Byte weicht der Stick ab?
/// (Nur im Fehlerfall, deshalb darf es langsam sein.)
fn find_first_difference<D: Device>(dev: &mut D, job: &FlashJob, written: u64) -> io::Result<u64> {
    let file = File::open(&job.image)?;
    let mut src: Box<dyn Read> = match job.compression {
        Compression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(file))),
        _ => Box::new(BufReader::new(file)),
    };
    dev.seek(SeekFrom::Start(0))?;
    let mut a = AlignedBuf::new(CHUNK);
    let mut b = vec![0u8; CHUNK];
    let mut pos = 0u64;
    while pos < written {
        let len = (written - pos).min(CHUNK as u64) as usize;
        dev.read_exact(&mut a[..round_up(len as u64) as usize])?;
        fill(&mut *src, &mut b[..len])?;
        if let Some(i) = a[..len].iter().zip(&b[..len]).position(|(x, y)| x != y) {
            return Ok(pos + i as u64);
        }
        pos += len as u64;
    }
    Ok(written)
}
