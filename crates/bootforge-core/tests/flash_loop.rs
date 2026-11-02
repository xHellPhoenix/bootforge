//! Echter Linux-Gerätezugriff mit einem Loop-Device (eine Datei, die der Kernel als Laufwerk
//! behandelt): Aushängen, exklusives Öffnen (O_EXCL), O_DIRECT, Zurücklesen.
//!
//! Braucht root und legt Kernel-Geräte an – läuft deshalb nur auf ausdrücklichen Wunsch:
//!
//!     sudo BOOTFORGE_LOOP_TEST=1 cargo test -p bootforge-core --test flash_loop -- --ignored
//!
//! Hinweis: Hier wird bewusst NICHT `flash_drive` benutzt, denn das lässt nur USB-Sticks zu
//! (Loop-Devices sind zu Recht verboten). Getestet wird der Teil darunter.
#![cfg(target_os = "linux")]

use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::AtomicBool;

use bootforge_core::catalog::{Compression, HashAlgorithm};
use bootforge_core::device::{Bus, Drive};
use bootforge_core::flash::linux::{finish, open};
use bootforge_core::flash::write::{verify_image, write_and_verify};
use bootforge_core::flash::{FlashError, FlashJob};
use bootforge_core::verify::sums::Hasher;

const MB: u64 = 1024 * 1024;

fn sh(cmd: &str, args: &[&str]) -> String {
    let out = Command::new(cmd).args(args).output().expect(cmd);
    assert!(
        out.status.success(),
        "{cmd} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn enabled() -> bool {
    std::env::var("BOOTFORGE_LOOP_TEST").is_ok()
}

struct Loop {
    dev: String,
}
impl Drop for Loop {
    fn drop(&mut self) {
        let _ = Command::new("losetup").args(["-d", &self.dev]).status();
    }
}

fn drive(dev: &str, mounts: Vec<String>) -> Drive {
    Drive {
        id: dev.into(),
        name: "Loop".into(),
        serial: None,
        size: 64 * MB,
        bus: Bus::Usb,
        removable: true,
        mounts,
        rejection: None,
    }
}

#[test]
#[ignore]
fn mounted_loop_device_is_unmounted_written_and_verified() {
    if !enabled() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("bf-loop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("mnt")).unwrap();
    let backing = dir.join("disk.img");
    sh("truncate", &["-s", "64M", backing.to_str().unwrap()]);
    let lo = Loop {
        dev: sh("losetup", &["-f", "--show", backing.to_str().unwrap()]),
    };

    // Wie ein Stick, der im Dateimanager geöffnet ist: Dateisystem + eingehängt.
    sh("mkfs.ext4", &["-q", &lo.dev]);
    let mnt = dir.join("mnt");
    sh("mount", &[&lo.dev, mnt.to_str().unwrap()]);
    std::fs::write(mnt.join("alte-datei.txt"), "weg damit").unwrap();

    // Image: 20 MB + krumme Bytes
    let content: Vec<u8> = (0..(20 * MB as usize + 333))
        .map(|i| (i * 7 % 251) as u8)
        .collect();
    let image = dir.join("x.iso");
    std::fs::write(&image, &content).unwrap();
    let mut h = Hasher::new(HashAlgorithm::Sha256);
    h.update(&content);
    let job = FlashJob {
        image,
        compression: Compression::None,
        algorithm: HashAlgorithm::Sha256,
        expected_hash: h.finish_hex(),
    };

    let no = AtomicBool::new(false);
    verify_image(&job, &no, &mut |_| {}).unwrap();
    let mut dev = open(&drive(&lo.dev, vec![mnt.to_string_lossy().into_owned()])).expect("öffnen");
    // Ausgehängt?
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap();
    assert!(!mounts.contains(&lo.dev), "noch eingehängt");

    let report = write_and_verify(&mut dev, 64 * MB, &job, &no, &mut |_| {}).expect("schreiben");
    finish(dev);
    assert_eq!(report.written, content.len() as u64);

    let disk = std::fs::read(&backing).unwrap();
    assert_eq!(&disk[..content.len()], &content[..], "Daten auf dem Gerät");
}

#[test]
#[ignore]
fn device_in_use_is_refused() {
    if !enabled() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("bf-loop-busy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let backing = dir.join("disk.img");
    sh("truncate", &["-s", "64M", backing.to_str().unwrap()]);
    let lo = Loop {
        dev: sh("losetup", &["-f", "--show", backing.to_str().unwrap()]),
    };

    // Ein anderes Programm hält das Gerät exklusiv offen.
    let _holder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EXCL)
        .open(Path::new(&lo.dev))
        .unwrap();
    match open(&drive(&lo.dev, vec![])) {
        Err(FlashError::Busy(_)) => {}
        other => panic!("erwartet Busy, war {:?}", other.err()),
    }
}

// --- Windows-Modus: ISO einbinden → GPT + FAT32 auf ein Loop-Device ---------------------------

use bootforge_core::flash::mount::{FileStamp, MountedIso};
use bootforge_core::flash::windows_media::{MediaOptions, write_windows_media};

fn have(tool: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {tool}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Wie viele Loop-Geräte hängen an dieser Datei? (Nach dem Aushängen müssen es 0 sein.)
fn loops_on(file: &Path) -> usize {
    sh("losetup", &["-j", file.to_str().unwrap()])
        .lines()
        .count()
}

/// Kleines Windows-artiges UDF-ISO, wie es Microsoft ausliefert (UDF + ISO9660-Brücke).
fn make_udf_iso(dir: &Path) -> std::path::PathBuf {
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("efi/boot")).unwrap();
    std::fs::create_dir_all(tree.join("sources")).unwrap();
    std::fs::write(tree.join("bootmgr"), vec![0x42u8; 400_000]).unwrap();
    std::fs::write(tree.join("bootmgr.efi"), vec![0x43u8; 1_500_000]).unwrap();
    std::fs::write(tree.join("efi/boot/bootx64.efi"), vec![0x44u8; 2_000_000]).unwrap();
    std::fs::write(tree.join("setup.exe"), b"MZ setup").unwrap();
    let wim: Vec<u8> = (0..3_000_000u32).map(|i| (i * 31 % 253) as u8).collect();
    std::fs::write(tree.join("sources/boot.wim"), wim).unwrap();
    let iso = dir.join("win.iso");
    sh(
        "genisoimage",
        &[
            "-quiet",
            "-udf",
            "-iso-level",
            "3",
            "-V",
            "CCCOMA_X64",
            "-o",
            iso.to_str().unwrap(),
            tree.to_str().unwrap(),
        ],
    );
    iso
}

#[test]
#[ignore]
fn changed_iso_is_not_mounted() {
    if !enabled() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("bf-loop-stamp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let iso = dir.join("x.iso");
    std::fs::write(&iso, b"geprueft").unwrap();
    let stamp = FileStamp::of(&iso).unwrap();
    // Nach der Prüfung ausgetauscht (neue Datei = neuer Inode)
    std::fs::remove_file(&iso).unwrap();
    std::fs::write(&iso, b"getauscht").unwrap();
    match MountedIso::mount(&iso, &stamp) {
        Err(FlashError::ImageChanged) => {}
        other => panic!("erwartet ImageChanged, war {:?}", other.err()),
    }
    assert_eq!(loops_on(&iso), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore]
fn windows_iso_mounted_and_written_to_loop_device() {
    if !enabled() {
        return;
    }
    if !have("genisoimage") {
        eprintln!("HINWEIS: genisoimage fehlt – Test übersprungen");
        return;
    }
    let dir = std::env::temp_dir().join(format!("bf-loop-win-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let iso = make_udf_iso(&dir);
    let stamp = FileStamp::of(&iso).unwrap();

    let mounted = match MountedIso::mount(&iso, &stamp) {
        Ok(m) => Some(m),
        Err(FlashError::Io(e)) if e.contains("vom Kernel nicht unterstützt") => {
            // Einbinden gescheitert: Das Loop-Gerät muss trotzdem wieder frei sein.
            assert_eq!(loops_on(&iso), 0, "Loop-Gerät nach Fehler nicht gelöst");
            eprintln!(
                "HINWEIS: Kernel ohne UDF/ISO9660 – Einbinden übersprungen, \
                 schreibe aus dem Quellverzeichnis"
            );
            None
        }
        Err(e) => panic!("einbinden: {e}"),
    };
    let source = mounted
        .as_ref()
        .map_or_else(|| dir.join("tree"), |m| m.root().to_owned());
    assert!(source.join("sources/boot.wim").is_file());

    let size = 128 * MB;
    let backing = dir.join("stick.img");
    sh("truncate", &["-s", "128M", backing.to_str().unwrap()]);
    let lo = Loop {
        dev: sh("losetup", &["-f", "--show", backing.to_str().unwrap()]),
    };
    let mut d = drive(&lo.dev, vec![]);
    d.size = size;
    let mut dev = open(&d).expect("öffnen");
    assert_eq!(dev.logical_sector_size().unwrap(), 512);

    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let opts = MediaOptions::new(work);
    let report = write_windows_media(
        &mut dev,
        size,
        &source,
        &opts,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .expect("Windows-Stick schreiben");
    finish(dev);
    assert!(report.written > 6_000_000);

    if let Some(m) = mounted {
        m.check_unchanged().unwrap();
        drop(m);
        // Aufgeräumt: ausgehängt, Loop-Gerät gelöst, Einhängepunkt gelöscht.
        let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap();
        assert!(!mounts.contains(source.to_str().unwrap()));
        assert!(!source.exists());
        assert_eq!(loops_on(&iso), 0);
    }

    // Gegenproben mit Standardwerkzeugen
    if have("sgdisk") {
        let out = Command::new("sgdisk")
            .args(["-v", backing.to_str().unwrap()])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("No problems found"), "{text}");
    }
    if have("fsck.vfat") {
        let part = Loop {
            dev: sh(
                "losetup",
                &[
                    "-f",
                    "--show",
                    "-r",
                    "-o",
                    &MB.to_string(),
                    "--sizelimit",
                    &(size - 2 * MB).to_string(),
                    backing.to_str().unwrap(),
                ],
            ),
        };
        let out = Command::new("fsck.vfat")
            .args(["-n", &part.dev])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
