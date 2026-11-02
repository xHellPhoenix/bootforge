//! Speicherplatz: freier Platz auf einem Laufwerk und Größe des Image-Caches.

use std::io;
use std::path::Path;

use serde::Serialize;

/// Puffer, der beim Download immer frei bleiben soll (das System braucht auch noch Platz).
pub const RESERVE: u64 = 256 * 1024 * 1024;

/// Freier Platz (für den aktuellen Benutzer) auf dem Laufwerk, auf dem `path` liegt.
/// `path` muss existieren.
#[cfg(unix)]
pub fn available_space(path: &Path) -> io::Result<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    // SAFETY: gültiger, nullterminierter Pfad; `st` wird von statvfs vollständig beschrieben.
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return Err(io::Error::last_os_error());
        }
        // f_bavail: Blöcke, die normale Benutzer nutzen dürfen (ohne die root-Reserve).
        Ok(st.f_bavail as u64 * st.f_frsize as u64)
    }
}

#[cfg(windows)]
pub fn available_space(path: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: nullterminierter UTF-16-Pfad; die beiden anderen Ausgaben dürfen NULL sein.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(free)
    }
}

/// Inhalt des Image-Caches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CacheInfo {
    /// Belegter Platz in Bytes (fertige Images + angefangene Downloads).
    pub bytes: u64,
    /// Anzahl fertiger Images.
    pub images: u32,
    /// Anzahl angefangener Downloads (`.part`).
    pub partial: u32,
    /// Freier Platz auf dem Laufwerk des Caches.
    pub available: Option<u64>,
}

/// Größe des Cache-Ordners. Fehlt er, ist er eben leer.
pub fn cache_info(dir: &Path) -> io::Result<CacheInfo> {
    let mut info = CacheInfo::default();
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            for e in entries {
                let e = e?;
                let meta = e.metadata()?;
                if !meta.is_file() {
                    continue;
                }
                info.bytes += meta.len();
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".part") {
                    info.partial += 1;
                } else if !name.ends_with(".part.json") && !name.ends_with(".sha256") {
                    // `.sha256`: festgehaltener Hash eines Downloads ohne Hersteller-Prüfsumme.
                    info.images += 1;
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    // Für den freien Platz den nächsten existierenden Ordner nehmen.
    let existing = dir.ancestors().find(|p| p.exists());
    info.available = existing.and_then(|p| available_space(p).ok());
    Ok(info)
}

/// Löscht alle Dateien im Cache-Ordner (nur Dateien direkt darin, keine Unterordner, keine Links).
pub fn clear_cache(dir: &Path) -> io::Result<u64> {
    let mut freed = 0;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    for e in entries {
        let e = e?;
        // symlink_metadata: einem Link nicht folgen – gelöscht wird nur, was wirklich hier liegt.
        let meta = std::fs::symlink_metadata(e.path())?;
        if meta.is_file() {
            std::fs::remove_file(e.path())?;
            freed += meta.len();
        }
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_counts_and_clears() {
        let dir = std::env::temp_dir().join(format!("bf-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            cache_info(&dir).unwrap().bytes,
            0,
            "fehlender Ordner = leer"
        );
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.iso"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("b.iso.part"), vec![0u8; 300]).unwrap();
        std::fs::write(dir.join("b.iso.part.json"), b"{}").unwrap();
        std::fs::write(dir.join("sub/keep.txt"), b"x").unwrap();

        let info = cache_info(&dir).unwrap();
        assert_eq!((info.bytes, info.images, info.partial), (1302, 1, 1));
        assert!(info.available.unwrap() > 0);

        assert_eq!(clear_cache(&dir).unwrap(), 1302);
        assert_eq!(cache_info(&dir).unwrap().bytes, 0);
        assert!(dir.join("sub/keep.txt").exists(), "Unterordner bleiben");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
