//! Linux: USB-Stick zum Schreiben vorbereiten und öffnen. Läuft im Root-Helper.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

use super::write::FlashError;
use crate::device::Drive;

/// `BLKRRPART` = `_IO(0x12, 95)`: Kernel soll die Partitionstabelle neu einlesen.
const BLKRRPART: libc::c_ulong = 0x125F;
/// `BLKSSZGET` = `_IO(0x12, 104)`: logische Sektorgröße.
const BLKSSZGET: libc::c_ulong = 0x1268;

pub struct LinuxDevice(File);

impl io::Read for LinuxDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}
impl io::Write for LinuxDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl io::Seek for LinuxDevice {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}
impl LinuxDevice {
    /// Logische Sektorgröße (fast immer 512, bei manchen USB-Festplatten 4096).
    pub fn logical_sector_size(&self) -> io::Result<u32> {
        let mut size: libc::c_int = 0;
        // SAFETY: gültiger Griff eines Blockgeräts; BLKSSZGET schreibt einen int.
        if unsafe { libc::ioctl(self.0.as_raw_fd(), BLKSSZGET as _, &mut size) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(size as u32)
    }
}

impl super::write::Device for LinuxDevice {
    fn sync(&mut self) -> io::Result<()> {
        self.0.sync_all()
    }
}

/// Partitionen aushängen und das Gerät exklusiv + am Cache vorbei öffnen.
pub fn open(drive: &Drive) -> Result<LinuxDevice, FlashError> {
    // SAFETY: geteuid hat keine Vorbedingungen.
    if unsafe { libc::geteuid() } != 0 {
        return Err(FlashError::NotElevated);
    }

    // Tiefere Einhängepunkte zuerst (/media/a/b vor /media/a).
    let mut mounts = drive.mounts.clone();
    mounts.sort_by_key(|m| std::cmp::Reverse(m.len()));
    for mp in &mounts {
        let c = CString::new(mp.as_str()).map_err(|_| FlashError::Busy(mp.clone()))?;
        // SAFETY: gültiger, nullterminierter Pfad.
        if unsafe { libc::umount2(c.as_ptr(), 0) } != 0 {
            let err = io::Error::last_os_error();
            return Err(FlashError::Busy(format!(
                "{mp} lässt sich nicht aushängen: {err}"
            )));
        }
    }

    // O_EXCL auf Blockgeräten: schlägt fehl, solange irgendetwas das Gerät benutzt
    // (eingehängt, LVM, …). Eine letzte, vom Kernel durchgesetzte Absicherung.
    // O_DIRECT: am Seitencache vorbei, damit das Zurücklesen wirklich vom Stick liest.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_EXCL | libc::O_DIRECT | libc::O_CLOEXEC)
        .open(&drive.id)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::EBUSY) => FlashError::Busy(format!("{} wird noch verwendet", drive.id)),
            _ => FlashError::Io(format!("{} lässt sich nicht öffnen: {e}", drive.id)),
        })?;
    Ok(LinuxDevice(file))
}

/// Nach dem Schreiben: Kernel liest die neue Partitionstabelle ein (Fehler sind egal).
pub fn finish(dev: LinuxDevice) {
    // SAFETY: gültiger Dateideskriptor eines Blockgeräts; BLKRRPART hat kein Argument.
    unsafe {
        libc::ioctl(dev.0.as_raw_fd(), BLKRRPART as _);
    }
}
