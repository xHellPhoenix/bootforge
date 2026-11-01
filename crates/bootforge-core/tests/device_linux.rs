//! Linux-Erkennung gegen einen nachgebauten /sys- und /proc-Baum.
//! Der „Testrechner“ hat absichtlich jede Menge Fallen – nur ein Laufwerk darf übrig bleiben.
#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use bootforge_core::device::{Bus, Rejection, ScanOptions, classify, linux::scan};

const GB: u64 = 1000 * 1000 * 1000;
const USB_BASE: &str = "sys/devices/pci0000:00/0000:00:14.0/usb2";

struct FakeSys {
    root: PathBuf,
    mountinfo: String,
}

impl FakeSys {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("bf-sys-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sys/block")).unwrap();
        fs::create_dir_all(root.join("proc/self")).unwrap();
        fs::write(
            root.join("proc/swaps"),
            "Filename Type Size Used Priority\n",
        )
        .unwrap();
        Self {
            root,
            mountinfo: String::new(),
        }
    }

    fn file(&self, rel: impl AsRef<Path>, content: &str) {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    /// Legt ein Blockgerät unter `parent` (Pfad im Gerätebaum) an und verlinkt es in /sys/block.
    fn disk(&self, parent: &str, name: &str, dev: &str, bytes: u64, removable: bool) -> PathBuf {
        let rel = format!("{parent}/block/{name}");
        let dir = self.root.join(&rel);
        fs::create_dir_all(&dir).unwrap();
        self.file(format!("{rel}/size"), &(bytes / 512).to_string());
        self.file(
            format!("{rel}/removable"),
            if removable { "1" } else { "0" },
        );
        self.file(format!("{rel}/ro"), "0");
        self.file(format!("{rel}/dev"), dev);
        symlink(&dir, self.root.join("sys/block").join(name)).unwrap();
        dir
    }

    /// USB-Stick: USB-Gerätordner mit Herstellerangaben + SCSI-Unterbau.
    fn usb(
        &self,
        port: &str,
        name: &str,
        dev: &str,
        bytes: u64,
        removable: bool,
        product: &str,
    ) -> PathBuf {
        let usb_dev = format!("{USB_BASE}/{port}");
        self.file(format!("{usb_dev}/idVendor"), "0781");
        self.file(format!("{usb_dev}/manufacturer"), "SanDisk");
        self.file(format!("{usb_dev}/product"), product);
        self.file(format!("{usb_dev}/serial"), &format!("SERIAL-{name}"));
        self.disk(
            &format!("{usb_dev}/{port}:1.0/host6/target6:0:0/6:0:0:0"),
            name,
            dev,
            bytes,
            removable,
        )
    }

    fn part(&self, disk: &Path, name: &str, dev: &str) -> PathBuf {
        let p = disk.join(name);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("partition"), "1").unwrap();
        fs::write(p.join("dev"), dev).unwrap();
        p
    }

    fn mount(&mut self, dev: &str, at: &str) {
        let n = self.mountinfo.lines().count() + 30;
        self.mountinfo.push_str(&format!(
            "{n} 1 {dev} / {at} rw,relatime - ext4 /dev/x rw\n"
        ));
    }

    fn finish(&self) -> &Path {
        fs::write(self.root.join("proc/self/mountinfo"), &self.mountinfo).unwrap();
        &self.root
    }
}

#[test]
fn only_the_real_stick_survives() {
    let mut s = FakeSys::new("laptop");

    // sda: interne SATA-SSD mit dem laufenden System + Swap
    let sda = s.disk(
        "sys/devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0",
        "sda",
        "8:0",
        512 * GB,
        false,
    );
    s.file("sys/block/sda/device/model", "Samsung SSD 870");
    s.part(&sda, "sda1", "8:1");
    s.part(&sda, "sda2", "8:2");
    s.part(&sda, "sda3", "8:3");
    s.mount("8:1", "/boot/efi");
    s.mount("8:2", "/");
    s.file(
        "proc/swaps",
        "Filename Type Size Used Priority\n/dev/sda3 partition 8388604 0 -2\n",
    );

    // nvme0n1: zweite interne Platte, gemountet als /data (kein Systempfad, aber nicht USB)
    let nvme = s.disk(
        "sys/devices/pci0000:00/0000:00:1d.0/nvme/nvme0",
        "nvme0n1",
        "259:0",
        1000 * GB,
        false,
    );
    s.part(&nvme, "nvme0n1p1", "259:1");
    s.mount("259:1", "/data");

    // sdb: DER Stick – 32 GB, mit gemounteter Partition (darf trotzdem gewählt werden)
    let sdb = s.usb("2-1", "sdb", "8:16", 32 * GB, true, "Ultra");
    s.part(&sdb, "sdb1", "8:17");
    s.mount("8:17", "/media/user/STICK");

    // sdc: externe USB-Festplatte, 1 TB, nicht „removable“
    s.usb("2-2", "sdc", "8:32", 1000 * GB, false, "Elements");

    // sdd: Live-Linux läuft gerade VOM USB-Stick
    let sdd = s.usb("2-3", "sdd", "8:48", 16 * GB, true, "Cruzer");
    s.part(&sdd, "sdd1", "8:49");
    s.mount("8:49", "/");

    // sde: USB-SSD mit LUKS, /home liegt darauf (über dm-0)
    let sde = s.usb("2-4", "sde", "8:64", 64 * GB, false, "Extreme");
    let sde1 = s.part(&sde, "sde1", "8:65");
    fs::create_dir_all(sde1.join("holders/dm-0")).unwrap();
    s.file("sys/block/dm-0/dev", "253:0");
    s.mount("253:0", "/home");

    // sdf: USB-Stick mit LVM, aber nichts davon gemountet → belegt, nicht System
    let sdf = s.usb("2-5", "sdf", "8:80", 32 * GB, true, "Glide");
    let sdf1 = s.part(&sdf, "sdf1", "8:81");
    fs::create_dir_all(sdf1.join("holders/dm-1")).unwrap();
    s.file("sys/block/dm-1/dev", "253:1");

    // sdg: USB-Kartenleser ohne Karte
    s.usb("2-6", "sdg", "8:96", 0, true, "Card Reader");

    // mmcblk0: eingebauter SD-Slot; loop0: virtuelles Gerät (wird gar nicht gelistet)
    s.disk(
        "sys/devices/pci0000:00/0000:00:1c.0/mmc_host/mmc0/mmc0:0001",
        "mmcblk0",
        "179:0",
        64 * GB,
        true,
    );
    s.disk("sys/devices/virtual", "loop0", "7:0", 10 * GB, false);

    let root = s.finish();
    let raw = scan(root).unwrap();
    let drives: Vec<_> = raw
        .iter()
        .map(|r| classify(r, ScanOptions::default()))
        .collect();
    let get = |id: &str| {
        drives
            .iter()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} fehlt"))
    };

    // Genau ein Laufwerk ist wählbar:
    let eligible: Vec<_> = drives
        .iter()
        .filter(|d| d.eligible())
        .map(|d| d.id.as_str())
        .collect();
    assert_eq!(eligible, ["/dev/sdb"]);

    // … und zwar mit den richtigen Daten:
    let sdb = get("/dev/sdb");
    assert_eq!(sdb.name, "SanDisk Ultra");
    assert_eq!(sdb.size, 32 * GB);
    assert_eq!(sdb.serial.as_deref(), Some("SERIAL-sdb"));
    assert_eq!(sdb.mounts, ["/media/user/STICK"]);

    // Alle anderen mit der richtigen Begründung:
    assert!(
        matches!(&get("/dev/sda").rejection, Some(Rejection::System(r)) if r.contains("/boot/efi"))
    );
    assert_eq!(
        get("/dev/nvme0n1").rejection,
        Some(Rejection::NotUsb(Bus::Nvme))
    );
    assert_eq!(get("/dev/sdc").rejection, Some(Rejection::ExternalHdd));
    assert!(
        matches!(&get("/dev/sdd").rejection, Some(Rejection::System(r)) if r.contains("als /"))
    );
    assert!(
        matches!(&get("/dev/sde").rejection, Some(Rejection::System(r)) if r.contains("dm-0") && r.contains("/home"))
    );
    assert!(matches!(&get("/dev/sdf").rejection, Some(Rejection::InUse(r)) if r.contains("dm-1")));
    assert_eq!(get("/dev/sdg").rejection, Some(Rejection::NoMedia));
    assert_eq!(
        get("/dev/mmcblk0").rejection,
        Some(Rejection::NotUsb(Bus::Mmc))
    );
    assert!(
        drives.iter().all(|d| d.id != "/dev/loop0"),
        "virtuelle Geräte nicht auflisten"
    );

    // Mit Option „externe Festplatten anzeigen“ wird sdc wählbar – sonst nichts.
    let with_hdd: Vec<_> = raw
        .iter()
        .map(|r| {
            classify(
                r,
                ScanOptions {
                    show_external_hdds: true,
                },
            )
        })
        .filter(|d| d.eligible())
        .map(|d| d.id)
        .collect();
    assert_eq!(with_hdd, ["/dev/sdb", "/dev/sdc"]);
}

#[test]
fn swap_on_usb_counts_as_system() {
    let mut s = FakeSys::new("swap");
    let sdb = s.usb("2-1", "sdb", "8:16", 8 * GB, true, "Stick");
    s.part(&sdb, "sdb1", "8:17");
    s.file(
        "proc/swaps",
        "Filename Type Size Used Priority\n/dev/sdb1 partition 1 0 -2\n",
    );
    s.mount("1:1", "/irgendwas");
    let raw = scan(s.finish()).unwrap();
    assert!(matches!(
        classify(&raw[0], ScanOptions::default()).rejection,
        Some(Rejection::System(_))
    ));
}

#[test]
fn usb_device_named_unexpectedly_is_rejected() {
    // USB-Pfad, aber kein sdX-Name (z. B. ein exotischer Treiber) → zweite Absicherung greift.
    let s = FakeSys::new("odd");
    s.disk(
        &format!("{USB_BASE}/2-1/2-1:1.0"),
        "nbd-usb0",
        "43:0",
        8 * GB,
        true,
    );
    s.disk(
        &format!("{USB_BASE}/2-2/2-2:1.0"),
        "xyz0",
        "44:0",
        8 * GB,
        true,
    );
    let raw = scan(s.finish()).unwrap();
    let d = classify(
        raw.iter().find(|r| r.id == "/dev/xyz0").unwrap(),
        ScanOptions::default(),
    );
    assert!(
        matches!(d.rejection, Some(Rejection::Unexpected(_))),
        "{d:?}"
    );
}

#[test]
fn this_machine_scans_without_error() {
    // Echter Lauf auf dem Rechner, auf dem die Tests laufen: darf nicht abstürzen.
    // (In CI und Cloud-Containern gibt es keine USB-Sticks – deshalb keine Aussage über Treffer.)
    let raw = scan(Path::new("/")).unwrap();
    for r in &raw {
        let d = classify(r, ScanOptions::default());
        if d.bus != Bus::Usb {
            assert!(
                !d.eligible(),
                "{} ist nicht USB und darf nie wählbar sein",
                d.id
            );
        }
    }
}
