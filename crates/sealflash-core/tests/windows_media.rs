//! Windows-Stick in eine Datei schreiben und mit unabhängigen Werkzeugen prüfen:
//! `sgdisk` (GPT), `fsck.vfat` (FAT32), `wimlib-imagex` (aufgeteilte WIM).
//!
//! Fehlen die Werkzeuge, werden diese Zusatzprüfungen übersprungen (mit Hinweis) – die
//! eingebaute Zurücklese-Prüfung von SealFlash läuft trotzdem.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use sealflash_core::flash::windows_media::{MediaOptions, find_wimlib, write_windows_media};
use sealflash_core::flash::{FlashError, FlashEvent, FlashStage};

const MB: u64 = 1024 * 1024;

fn have(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok()
        || Command::new(tool).arg("-V").output().is_ok()
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bf-win-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

/// Nachgebaute Windows-ISO (als Verzeichnis, so wie sie eingebunden aussieht).
fn fake_iso(root: &Path, with_wim: bool) {
    fs::create_dir_all(root.join("efi/boot")).unwrap();
    fs::create_dir_all(root.join("sources")).unwrap();
    fs::create_dir_all(root.join("support/Support Tools")).unwrap();
    fs::write(root.join("bootmgr"), noise(400_000, 1)).unwrap();
    fs::write(root.join("efi/boot/bootx64.efi"), noise(2_000_000, 2)).unwrap();
    fs::write(root.join("sources/boot.wim"), noise(700_000, 3)).unwrap();
    fs::write(
        root.join("support/Support Tools/Datei mit langem Namen.txt"),
        b"Hallo",
    )
    .unwrap();
    fs::write(root.join("setup.exe"), noise(100_000, 4)).unwrap();
    if with_wim {
        // Echte WIM aus drei Dateien à 1 MB (zufällig, nicht komprimierbar), damit `split`
        // wirklich aufteilt – eine einzelne Datei kann wimlib nicht über Teile verteilen.
        let content = root.parent().unwrap().join("wim-content");
        fs::create_dir_all(&content).unwrap();
        for i in 0..3 {
            fs::write(
                content.join(format!("payload{i}.bin")),
                noise(MB as usize, 5 + i),
            )
            .unwrap();
        }
        let ok = Command::new("wimlib-imagex")
            .args([
                "capture",
                content.to_str().unwrap(),
                root.join("sources/install.wim").to_str().unwrap(),
                "--compress=none",
            ])
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "wimlib-imagex capture");
    }
}

fn stick(d: &Path, size: u64) -> File {
    let p = d.join("stick.img");
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&p)
        .unwrap();
    f.set_len(size).unwrap();
    f
}

#[test]
fn creates_bootable_layout_and_splits_wim() {
    if find_wimlib().is_none() {
        eprintln!("HINWEIS: wimlib-imagex fehlt – Test übersprungen (Paket „wimtools“)");
        return;
    }
    let d = dir("full");
    let iso = d.join("iso");
    fake_iso(&iso, true);
    let size = 256 * MB;
    let dev = stick(&d, size);

    let mut opts = MediaOptions::new(d.join("tmp"));
    fs::create_dir_all(&opts.temp_dir).unwrap();
    opts.split_above = MB; // statt 4 GB
    opts.split_part_mib = 1;
    let mut stages = Vec::new();
    let report = write_windows_media(dev, size, &iso, &opts, &AtomicBool::new(false), &mut |e| {
        if let FlashEvent::Stage { stage } = e {
            stages.push(stage)
        }
    })
    .unwrap();
    assert_eq!(
        stages,
        [
            FlashStage::Split,
            FlashStage::Partition,
            FlashStage::Format,
            FlashStage::Copy,
            FlashStage::Readback
        ]
    );
    assert!(report.written > 3 * MB);

    let img = d.join("stick.img");

    // --- GPT mit sgdisk prüfen ---
    if have("sgdisk") {
        let out = Command::new("sgdisk")
            .args(["-v", img.to_str().unwrap()])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("No problems found"), "sgdisk -v:\n{text}");
        let info = String::from_utf8_lossy(
            &Command::new("sgdisk")
                .args(["-i", "1", img.to_str().unwrap()])
                .output()
                .unwrap()
                .stdout,
        )
        .into_owned();
        assert!(
            info.contains("EBD0A0A2-B9E5-4433-87C0-68B6B72699C7"),
            "Typ Basic Data:\n{info}"
        );
        assert!(info.contains("First sector: 2048"), "{info}");
    } else {
        eprintln!("HINWEIS: sgdisk fehlt – GPT-Gegenprobe übersprungen");
    }

    // --- Partition herausschneiden (ab 1 MiB, bis vor die Backup-GPT) ---
    let mut whole = Vec::new();
    File::open(&img).unwrap().read_to_end(&mut whole).unwrap();
    let part_end = ((size / 512 - 34) / 2048 * 2048) * 512;
    let part = &whole[MB as usize..part_end as usize];
    let part_img = d.join("part.img");
    fs::write(&part_img, part).unwrap();

    if have("fsck.vfat") {
        let out = Command::new("fsck.vfat")
            .args(["-n", "-v", part_img.to_str().unwrap()])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "fsck.vfat:\n{text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(text.contains("32 bit entries"), "FAT32 erwartet:\n{text}");
    } else {
        eprintln!("HINWEIS: fsck.vfat fehlt – FAT-Gegenprobe übersprungen");
    }

    // --- Inhalt unabhängig über fatfs aus dem Abbild lesen und mit der Quelle vergleichen ---
    let fs_ =
        fatfs::FileSystem::new(File::open(&part_img).unwrap(), fatfs::FsOptions::new()).unwrap();
    let root = fs_.root_dir();
    for rel in [
        "bootmgr",
        "efi/boot/bootx64.efi",
        "sources/boot.wim",
        "setup.exe",
        "support/Support Tools/Datei mit langem Namen.txt",
    ] {
        let mut got = Vec::new();
        root.open_file(rel).unwrap().read_to_end(&mut got).unwrap();
        assert_eq!(got, fs::read(iso.join(rel)).unwrap(), "{rel}");
    }
    assert!(
        root.open_file("sources/install.wim").is_err(),
        "die große WIM darf nicht 1:1 kopiert sein"
    );

    // --- aufgeteilte WIM: Teile herauskopieren und mit wimlib prüfen ---
    let swm_dir = d.join("swm");
    fs::create_dir_all(&swm_dir).unwrap();
    let mut parts = 0;
    for e in root.open_dir("sources").unwrap().iter() {
        let e = e.unwrap();
        let name = e.file_name();
        if name.to_lowercase().ends_with(".swm") {
            let mut data = Vec::new();
            e.to_file().read_to_end(&mut data).unwrap();
            fs::write(swm_dir.join(&name), data).unwrap();
            parts += 1;
        }
    }
    assert!(parts >= 3, "erwartet mehrere Teile, gefunden {parts}");
    let joined = d.join("joined.wim");
    let ok = Command::new("wimlib-imagex")
        .arg("join")
        .arg(&joined)
        .args(fs::read_dir(&swm_dir).unwrap().map(|e| e.unwrap().path()))
        .status()
        .unwrap()
        .success();
    assert!(ok, "Teile lassen sich wieder zusammensetzen");
    let out = Command::new("wimlib-imagex")
        .args(["verify", joined.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "wimlib verify: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn refuses_non_windows_source() {
    let d = dir("nonwin");
    let iso = d.join("iso");
    fs::create_dir_all(&iso).unwrap();
    fs::write(iso.join("vmlinuz"), b"linux").unwrap();
    let r = write_windows_media(
        stick(&d, 64 * MB),
        64 * MB,
        &iso,
        &MediaOptions::new(d.clone()),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(matches!(r, Err(FlashError::Unsupported(_))), "{r:?}");
}

#[test]
fn too_small_stick_rejected_before_formatting() {
    let d = dir("small");
    let iso = d.join("iso");
    fake_iso(&iso, false);
    fs::write(iso.join("sources/big.bin"), noise(40 * MB as usize, 9)).unwrap();
    let mut dev = stick(&d, 32 * MB);
    dev.write_all(b"ALTE DATEN").unwrap();
    let r = write_windows_media(
        dev,
        32 * MB,
        &iso,
        &MediaOptions::new(d.clone()),
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(matches!(r, Err(FlashError::TooSmall { .. })), "{r:?}");
}

#[test]
fn cancel_leaves_no_partition_table() {
    let d = dir("cancel");
    let iso = d.join("iso");
    fake_iso(&iso, false);
    let r = write_windows_media(
        stick(&d, 64 * MB),
        64 * MB,
        &iso,
        &MediaOptions::new(d.clone()),
        &AtomicBool::new(true),
        &mut |_| {},
    );
    assert_eq!(r, Err(FlashError::Cancelled));
    let mut head = vec![0u8; 1024];
    let mut f = File::open(d.join("stick.img")).unwrap();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.read_exact(&mut head).unwrap();
    assert!(
        head.iter().all(|&b| b == 0),
        "keine (halbe) Partitionstabelle"
    );
}
