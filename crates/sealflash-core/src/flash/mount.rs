//! ISO-Datei schreibgeschützt einbinden, damit der Windows-Modus ihre Dateien kopieren kann.
//!
//! - **Linux** (im Root-Helper): ohne externe Programme. Wir öffnen die Datei selbst
//!   (`O_NOFOLLOW`, nur lesen), hängen genau diesen Dateigriff an ein Loop-Gerät
//!   (`LOOP_SET_FD`) und binden das mit `mount(2)` ein – `ro,nosuid,nodev,noexec`, in ein
//!   privates Verzeichnis (nur root, Modus 0700). So kann niemand zwischen Prüfung und Einbinden
//!   die Datei hinter dem Pfad austauschen.
//! - **Windows**: `Mount-DiskImage` (PowerShell, ab Windows 8 eingebaut), schreibgeschützt.
//!
//! In beiden Fällen merkt sich [`FileStamp`] Größe und Änderungszeit (unter Linux auch Gerät und
//! Inode) vom Zeitpunkt der Prüfsummenprüfung. Weicht die eingebundene Datei davon ab – vorher
//! oder nach dem Kopieren –, wird abgebrochen: Es darf nur auf den Stick, was geprüft wurde.

use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::write::FlashError;

/// Fingerabdruck einer Datei (keine Prüfsumme – nur „ist es noch dieselbe, unveränderte Datei?“).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}

impl FileStamp {
    pub fn of(path: &Path) -> Result<Self, FlashError> {
        let meta = std::fs::metadata(path)
            .map_err(|e| FlashError::Io(format!("{}: {e}", path.display())))?;
        Ok(Self::from_meta(&meta))
    }

    fn from_meta(meta: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            size: meta.len(),
            modified: meta.modified().ok(),
            #[cfg(unix)]
            dev: meta.dev(),
            #[cfg(unix)]
            ino: meta.ino(),
        }
    }
}

/// Eigenes, leeres Arbeitsverzeichnis (nur für uns lesbar), wird am Ende samt Inhalt gelöscht.
///
/// Warum nicht einfach `/tmp/sealflash`? Der Linux-Helper läuft als root. Läge das Verzeichnis
/// unter einem festen Namen, könnte ein anderer Benutzer es vorher anlegen und Symlinks
/// hineinlegen – root würde dann beim Schreiben fremde Dateien überschreiben.
/// `create_dir` (schlägt fehl, wenn es den Namen schon gibt) + Modus 0700 verhindern das.
pub struct PrivateDir(PathBuf);

impl PrivateDir {
    pub fn new(base: &Path, prefix: &str) -> io::Result<Self> {
        for attempt in 0..32u32 {
            let g = super::gpt::random_guid();
            let name = format!(
                "{prefix}-{}-{attempt}",
                g[..6]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            let path = base.join(name);
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
            match b.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other(
            "kein freier Name für ein Arbeitsverzeichnis",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Wo große Zwischendateien (aufgeteilte install.wim, ~5 GB) hin dürfen.
/// Linux: `/var/tmp` liegt auf der Platte – `/tmp` ist oft eine RAM-Disk (tmpfs).
pub fn work_base() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        let p = PathBuf::from("/var/tmp");
        if p.is_dir() {
            return p;
        }
    }
    std::env::temp_dir()
}

pub use imp::MountedIso;

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    use super::{FileStamp, FlashError, PrivateDir};

    // ioctl-Nummern aus <linux/loop.h>
    const LOOP_CTL_GET_FREE: libc::c_ulong = 0x4C82;
    const LOOP_SET_FD: libc::c_ulong = 0x4C00;
    const LOOP_CLR_FD: libc::c_ulong = 0x4C01;
    const LOOP_SET_STATUS64: libc::c_ulong = 0x4C04;
    /// Loop-Gerät löst sich selbst, sobald das Dateisystem ausgehängt ist.
    const LO_FLAGS_AUTOCLEAR: u32 = 4;
    /// `struct loop_info64` ist 232 Bytes groß, `lo_flags` steht an Byte 52.
    const LOOP_INFO64_SIZE: usize = 232;
    const LO_FLAGS_OFFSET: usize = 52;

    pub struct MountedIso {
        /// Einhängepunkt – wird nach dem Aushängen (in `drop`) mitgelöscht.
        _dir: PrivateDir,
        root: std::path::PathBuf,
        /// Offener Griff auf die ISO – für die Prüfung „unverändert?“ am Ende.
        image: File,
        stamp: FileStamp,
        mounted: bool,
    }

    fn os(what: &str) -> FlashError {
        FlashError::Io(format!("{what}: {}", io::Error::last_os_error()))
    }

    /// Prüft, dass die Datei dem aufrufenden Benutzer gehört (wenn über pkexec gestartet).
    fn check_owner(meta: &std::fs::Metadata) -> Result<(), FlashError> {
        if let Ok(uid) = std::env::var("PKEXEC_UID") {
            let uid: u32 = uid
                .parse()
                .map_err(|_| FlashError::Io("PKEXEC_UID ungültig".into()))?;
            if meta.uid() != uid {
                return Err(FlashError::Io(
                    "Image gehört nicht dem aufrufenden Benutzer".into(),
                ));
            }
        }
        Ok(())
    }

    impl MountedIso {
        pub fn mount(image: &Path, expected: &FileStamp) -> Result<Self, FlashError> {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(image)
                .map_err(|e| FlashError::Io(format!("{}: {e}", image.display())))?;
            let meta = file.metadata().map_err(FlashError::from)?;
            if !meta.file_type().is_file() {
                return Err(FlashError::Io("Image ist keine normale Datei".into()));
            }
            check_owner(&meta)?;
            if FileStamp::from_meta(&meta) != *expected {
                return Err(FlashError::ImageChanged);
            }

            // /run gehört root und ist für andere nicht beschreibbar – ideal für den Einhängepunkt.
            let base = if Path::new("/run").is_dir() {
                Path::new("/run")
            } else {
                Path::new("/tmp")
            };
            let dir = PrivateDir::new(base, "sealflash-iso").map_err(FlashError::from)?;
            let loopdev = attach_loop(&file)?;
            let mut iso = Self {
                root: dir.path().to_owned(),
                _dir: dir,
                image: file,
                stamp: expected.clone(),
                mounted: false,
            };
            let src = CString::new(loopdev.as_bytes()).expect("kein NUL");
            let target = CString::new(iso.root.as_os_str().as_bytes())
                .map_err(|_| FlashError::Io("Pfad mit NUL".into()))?;
            let flags = libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC;
            // Windows-ISOs sind UDF (mit ISO9660 nur als Notbehelf für alte Systeme – dort fehlen
            // Dateien über 4 GB). Deshalb UDF zuerst.
            let mut errors = Vec::new();
            for fs in ["udf", "iso9660"] {
                let fstype = CString::new(fs).expect("kein NUL");
                // SAFETY: alle Zeiger sind gültige, nullterminierte Zeichenketten.
                let r = unsafe {
                    libc::mount(
                        src.as_ptr(),
                        target.as_ptr(),
                        fstype.as_ptr(),
                        flags,
                        std::ptr::null(),
                    )
                };
                if r == 0 {
                    iso.mounted = true;
                    return Ok(iso);
                }
                let e = io::Error::last_os_error();
                errors.push(if e.raw_os_error() == Some(libc::ENODEV) {
                    format!("{fs}: vom Kernel nicht unterstützt")
                } else {
                    format!("{fs}: {e}")
                });
            }
            // Einbinden fehlgeschlagen → Loop-Gerät wieder lösen.
            if let Ok(l) = File::open(&loopdev) {
                // SAFETY: gültiger Griff eines Loop-Geräts, LOOP_CLR_FD hat kein Argument.
                unsafe { libc::ioctl(l.as_raw_fd(), LOOP_CLR_FD as _, 0) };
            }
            Err(FlashError::Io(format!(
                "ISO lässt sich nicht einbinden ({})",
                errors.join("; ")
            )))
        }

        pub fn root(&self) -> &Path {
            &self.root
        }

        /// Ist die ISO seit der Prüfung unverändert?
        pub fn check_unchanged(&self) -> Result<(), FlashError> {
            let meta = self.image.metadata().map_err(FlashError::from)?;
            if FileStamp::from_meta(&meta) == self.stamp {
                Ok(())
            } else {
                Err(FlashError::ImageChanged)
            }
        }
    }

    /// Freies Loop-Gerät holen und unseren Dateigriff daran hängen (nur lesend, löst sich selbst).
    fn attach_loop(file: &File) -> Result<std::ffi::OsString, FlashError> {
        let ctl = File::open("/dev/loop-control").map_err(|e| {
            FlashError::Io(format!(
                "/dev/loop-control: {e} (Kernelmodul „loop“ geladen?)"
            ))
        })?;
        // Ein anderes Programm kann uns das „freie“ Gerät wegschnappen → ein paar Versuche.
        for _ in 0..10 {
            // SAFETY: gültiger Griff, LOOP_CTL_GET_FREE hat kein Argument.
            let n = unsafe { libc::ioctl(ctl.as_raw_fd(), LOOP_CTL_GET_FREE as _) };
            if n < 0 {
                return Err(os("kein freies Loop-Gerät"));
            }
            let path = format!("/dev/loop{n}");
            let dev = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_CLOEXEC)
                .open(&path)
                .map_err(|e| FlashError::Io(format!("{path}: {e}")))?;
            // SAFETY: beides gültige Griffe; LOOP_SET_FD nimmt den Dateideskriptor als Wert.
            if unsafe { libc::ioctl(dev.as_raw_fd(), LOOP_SET_FD as _, file.as_raw_fd()) } != 0 {
                if io::Error::last_os_error().raw_os_error() == Some(libc::EBUSY) {
                    continue;
                }
                return Err(os("LOOP_SET_FD"));
            }
            let mut info = [0u8; LOOP_INFO64_SIZE];
            info[LO_FLAGS_OFFSET..LO_FLAGS_OFFSET + 4]
                .copy_from_slice(&LO_FLAGS_AUTOCLEAR.to_ne_bytes());
            // SAFETY: Puffer hat die Größe von `struct loop_info64`.
            if unsafe { libc::ioctl(dev.as_raw_fd(), LOOP_SET_STATUS64 as _, info.as_ptr()) } != 0 {
                let err = os("LOOP_SET_STATUS64");
                // SAFETY: wie oben.
                unsafe { libc::ioctl(dev.as_raw_fd(), LOOP_CLR_FD as _, 0) };
                return Err(err);
            }
            return Ok(path.into());
        }
        Err(FlashError::Io("kein freies Loop-Gerät".into()))
    }

    impl Drop for MountedIso {
        fn drop(&mut self) {
            if !self.mounted {
                return;
            }
            let Ok(target) = CString::new(self.root.as_os_str().as_bytes()) else {
                return;
            };
            // Kurz nach dem Kopieren kann der Kernel das Dateisystem noch als beschäftigt melden.
            for _ in 0..20 {
                // SAFETY: gültiger Pfad.
                if unsafe { libc::umount2(target.as_ptr(), 0) } == 0 {
                    self.mounted = false;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if self.mounted {
                // Letzter Ausweg: „lazy“ – wird ausgehängt, sobald niemand mehr zugreift.
                // SAFETY: gültiger Pfad.
                unsafe { libc::umount2(target.as_ptr(), libc::MNT_DETACH) };
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{FileStamp, FlashError};

    pub struct MountedIso {
        image: PathBuf,
        root: PathBuf,
        stamp: FileStamp,
    }

    /// Für PowerShell in einfache Anführungszeichen setzen (' wird verdoppelt).
    fn ps_quote(p: &Path) -> String {
        format!("'{}'", p.to_string_lossy().replace('\'', "''"))
    }

    fn powershell(script: &str) -> std::io::Result<std::process::Output> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
            ])
            .arg(script)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    impl MountedIso {
        pub fn mount(image: &Path, expected: &FileStamp) -> Result<Self, FlashError> {
            if FileStamp::of(image)? != *expected {
                return Err(FlashError::ImageChanged);
            }
            let q = ps_quote(image);
            // Laufwerksbuchstabe, sonst der Volume-Pfad (\\?\Volume{…}\), falls keine Buchstaben
            // automatisch vergeben werden.
            let script = format!(
                "$ErrorActionPreference='Stop'; \
                 $v = Mount-DiskImage -ImagePath {q} -StorageType ISO -Access ReadOnly -PassThru | Get-Volume; \
                 if ($v.DriveLetter) {{ \"$($v.DriveLetter):\\\" }} else {{ $v.Path }}"
            );
            let out = powershell(&script)
                .map_err(|e| FlashError::Io(format!("PowerShell lässt sich nicht starten: {e}")))?;
            let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !out.status.success() || text.is_empty() {
                // Falls doch eingebunden: wieder lösen.
                let _ = powershell(&format!("Dismount-DiskImage -ImagePath {q}"));
                return Err(FlashError::Io(format!(
                    "ISO lässt sich nicht einbinden: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )));
            }
            Ok(Self {
                image: image.to_owned(),
                root: PathBuf::from(text.lines().last().unwrap_or("").trim()),
                stamp: expected.clone(),
            })
        }

        pub fn root(&self) -> &Path {
            &self.root
        }

        pub fn check_unchanged(&self) -> Result<(), FlashError> {
            if FileStamp::of(&self.image)? == self.stamp {
                Ok(())
            } else {
                Err(FlashError::ImageChanged)
            }
        }
    }

    impl Drop for MountedIso {
        fn drop(&mut self) {
            let _ = powershell(&format!(
                "Dismount-DiskImage -ImagePath {}",
                ps_quote(&self.image)
            ));
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use std::path::Path;

    use super::{FileStamp, FlashError};

    pub struct MountedIso;

    impl MountedIso {
        pub fn mount(_image: &Path, _expected: &FileStamp) -> Result<Self, FlashError> {
            Err(FlashError::Unsupported("ISO einbinden".into()))
        }
        pub fn root(&self) -> &Path {
            Path::new("")
        }
        pub fn check_unchanged(&self) -> Result<(), FlashError> {
            Ok(())
        }
    }
}
