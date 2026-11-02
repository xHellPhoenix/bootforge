//! Windows: USB-Stick zum Schreiben vorbereiten und öffnen.
//!
//! Ablauf wie bei Rufus im „DD-Modus“:
//! 1. Prüfen, ob wir Administratorrechte haben.
//! 2. Jedes Volume auf dem Stick öffnen, **sperren** (`FSCTL_LOCK_VOLUME`) und **aushängen**
//!    (`FSCTL_DISMOUNT_VOLUME`). Die Griffe bleiben bis zum Ende offen – sonst hängt Windows
//!    das Volume sofort wieder ein und blockiert das Schreiben.
//! 3. `\\.\PhysicalDriveN` mit `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH` öffnen
//!    (am Cache vorbei, damit das Zurücklesen wirklich vom Stick kommt).
//! 4. Nach dem Schreiben `IOCTL_DISK_UPDATE_PROPERTIES`, damit Windows die neue Partitionstabelle sieht.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_NO_BUFFERING, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, IOCTL_DISK_GET_DRIVE_GEOMETRY,
    IOCTL_DISK_UPDATE_PROPERTIES,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::write::FlashError;
use crate::device::Drive;

pub struct WindowsDevice {
    disk: File,
    /// Gesperrte Volumes – müssen offen bleiben, bis wir fertig sind.
    _volumes: Vec<File>,
}

impl io::Read for WindowsDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.disk.read(buf)
    }
}
impl io::Write for WindowsDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.disk.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.disk.flush()
    }
}
impl io::Seek for WindowsDevice {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.disk.seek(pos)
    }
}
impl WindowsDevice {
    /// Logische Sektorgröße (fast immer 512, bei manchen USB-Festplatten 4096).
    pub fn logical_sector_size(&self) -> io::Result<u32> {
        // DISK_GEOMETRY: Cylinders (8), MediaType (4), TracksPerCylinder (4),
        // SectorsPerTrack (4), BytesPerSector (4) = 24 Bytes.
        let mut geo = [0u8; 24];
        let mut returned = 0u32;
        // SAFETY: gültiges Handle, Ausgabepuffer mit der Größe von DISK_GEOMETRY.
        let ok = unsafe {
            DeviceIoControl(
                self.disk.as_raw_handle(),
                IOCTL_DISK_GET_DRIVE_GEOMETRY,
                std::ptr::null(),
                0,
                geo.as_mut_ptr().cast(),
                geo.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(u32::from_le_bytes(geo[20..24].try_into().expect("4 Bytes")))
    }
}

impl super::write::Device for WindowsDevice {
    fn sync(&mut self) -> io::Result<()> {
        self.disk.sync_all()
    }
}

/// Läuft der Prozess mit Administratorrechten (erhöht)?
pub fn is_elevated() -> bool {
    // SAFETY: Standard-Win32-Aufrufe mit gültigen Zeigern; Handle wird geschlossen.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

fn ioctl(file: &File, code: u32) -> io::Result<()> {
    let mut returned = 0u32;
    // SAFETY: gültiges Handle; diese IOCTLs haben weder Ein- noch Ausgabepuffer.
    let ok = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            code,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn share_rw() -> u32 {
    FILE_SHARE_READ | FILE_SHARE_WRITE
}

/// Ein Volume sperren und aushängen. Sperren kann kurz scheitern, wenn gerade z. B. der
/// Explorer oder ein Virenscanner darauf zugreift – deshalb einige Versuche (wie Rufus).
fn lock_and_dismount(path: &str) -> Result<File, FlashError> {
    let vol = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(share_rw())
        .open(path)
        .map_err(|e| FlashError::Busy(format!("{path}: {e}")))?;
    let mut last = None;
    for _ in 0..20 {
        match ioctl(&vol, FSCTL_LOCK_VOLUME) {
            Ok(()) => {
                last = None;
                break;
            }
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
    if let Some(e) = last {
        return Err(FlashError::Busy(format!(
            "{path} lässt sich nicht sperren – ist darauf noch eine Datei geöffnet? ({e})"
        )));
    }
    ioctl(&vol, FSCTL_DISMOUNT_VOLUME).map_err(|e| FlashError::Busy(format!("{path}: {e}")))?;
    Ok(vol)
}

/// `\\.\PhysicalDrive3` → 3
fn disk_number(id: &str) -> Option<u32> {
    id.strip_prefix(r"\\.\PhysicalDrive")?.parse().ok()
}

pub fn open(drive: &Drive) -> Result<WindowsDevice, FlashError> {
    if !is_elevated() {
        return Err(FlashError::NotElevated);
    }
    let number = disk_number(&drive.id)
        .ok_or_else(|| FlashError::Drive(format!("unerwartete ID {}", drive.id)))?;

    let mut volumes = Vec::new();
    let paths =
        crate::device::windows::volume_paths(number).map_err(|e| FlashError::Io(e.to_string()))?;
    for path in paths {
        volumes.push(lock_and_dismount(&path)?);
    }

    let disk = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(share_rw())
        .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH)
        .open(&drive.id)
        .map_err(|e| FlashError::Io(format!("{} lässt sich nicht öffnen: {e}", drive.id)))?;
    Ok(WindowsDevice {
        disk,
        _volumes: volumes,
    })
}

/// Windows soll die neue Partitionstabelle einlesen. Danach werden die Volumes freigegeben.
pub fn finish(dev: WindowsDevice) {
    let _ = ioctl(&dev.disk, IOCTL_DISK_UPDATE_PROPERTIES);
    drop(dev);
}

#[cfg(test)]
mod tests {
    #[test]
    fn disk_numbers() {
        assert_eq!(super::disk_number(r"\\.\PhysicalDrive1"), Some(1));
        assert_eq!(super::disk_number(r"\\.\PhysicalDriveX"), None);
        assert_eq!(super::disk_number("/dev/sdb"), None);
    }
}
