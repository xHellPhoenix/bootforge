//! Gesamtablauf: Prüfsumme → Signatur → Download → Vergleich → Cache.
//! Mit selbst erzeugtem Schlüssel, damit Manipulationen gezielt getestet werden können.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use openpgp::cert::CertBuilder;
use openpgp::policy::StandardPolicy;
use openpgp::serialize::SerializeInto;
use openpgp::serialize::stream::{Armorer, Message, Signer};
use sealflash_core::acquire::{AcquireError, AcquireEvent, Assurance, Stage, acquire};
use sealflash_core::catalog::{Compression, FlashMode, HashAlgorithm, SignatureKind, SignedTarget};
use sealflash_core::download::fake::FakeSource;
use sealflash_core::download::{Opened, RangeSource};
use sealflash_core::http::fake::FakeFetcher;
use sealflash_core::http::{Conditional, FetchError, Fetcher};
use sealflash_core::resolve::{ResolvedChecksum, ResolvedImage, ResolvedSignature};
use sealflash_core::verify::sums::Hasher;
use sequoia_openpgp as openpgp;
use url::Url;

const BASE: &str = "https://mirror.example/iso/";

/// Kleine Dateien (`Fetcher`) und der große Download (`RangeSource`) in einem.
struct Net {
    pages: FakeFetcher,
    image: FakeSource,
}

impl Fetcher for Net {
    async fn get(&self, url: &Url, etag: Option<&str>) -> Result<Conditional, FetchError> {
        self.pages.get(url, etag).await
    }
    async fn content_length(&self, url: &Url) -> Result<Option<u64>, FetchError> {
        self.pages.content_length(url).await
    }
}

impl RangeSource for Net {
    type Body = <FakeSource as RangeSource>::Body;
    async fn open(
        &self,
        url: &Url,
        from: u64,
        if_range: Option<&str>,
    ) -> Result<Opened<Self::Body>, FetchError> {
        self.image.open(url, from, if_range).await
    }
}

fn image_data() -> Vec<u8> {
    (0..100_000u32).map(|i| (i % 253) as u8).collect()
}

fn sha256(d: &[u8]) -> String {
    let mut h = Hasher::new(HashAlgorithm::Sha256);
    h.update(d);
    h.finish_hex()
}

fn url(path: &str) -> Url {
    format!("{BASE}{path}").parse().unwrap()
}

fn resolved(signature: Option<ResolvedSignature>) -> ResolvedImage {
    ResolvedImage {
        variant_id: "test".into(),
        version: "1.0".into(),
        build: None,
        filename: "test-1.0.iso".into(),
        url: url("test-1.0.iso"),
        size: Some(100_000),
        checksum: ResolvedChecksum::File {
            url: url("SHA256SUMS"),
            algorithm: HashAlgorithm::Sha256,
        },
        signature,
        compression: Compression::None,
        flash_mode: FlashMode::Raw,
    }
}

/// Erzeugt einen Schlüssel und gibt (öffentlicher Schlüssel als ASCII, Fingerprint, Signierfunktion) zurück.
fn keypair() -> (Vec<u8>, String, impl Fn(&[u8]) -> Vec<u8>) {
    let (cert, _) = CertBuilder::general_purpose(Some("Test Vendor <release@example.org>"))
        .generate()
        .unwrap();
    let fp = cert.fingerprint().to_hex();
    let public = cert.armored().to_vec().unwrap();
    let sign = move |data: &[u8]| {
        let policy = StandardPolicy::new();
        let key = cert
            .keys()
            .with_policy(&policy, None)
            .secret()
            .for_signing()
            .next()
            .unwrap()
            .key()
            .clone()
            .into_keypair()
            .unwrap();
        let mut out = Vec::new();
        let m = Message::new(&mut out);
        let m = Armorer::new(m)
            .kind(openpgp::armor::Kind::Signature)
            .build()
            .unwrap();
        let mut s = Signer::new(m, key).unwrap().detached().build().unwrap();
        s.write_all(data).unwrap();
        s.finalize().unwrap();
        out
    };
    (public, fp, sign)
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bf-acq-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn sig_meta(fp: &str, signs: SignedTarget, url_: &str) -> ResolvedSignature {
    ResolvedSignature {
        kind: SignatureKind::GpgDetached,
        signs,
        url: Some(url(url_)),
        fingerprints: vec![fp.into()],
        keyring_url: Some(url("key.asc")),
        allow_v3: false,
    }
}

#[tokio::test]
async fn checksum_only_download_then_cache_hit() {
    let data = image_data();
    let sums = format!("{}  test-1.0.iso\n", sha256(&data));
    let net = Net {
        pages: FakeFetcher::new().page(&format!("{BASE}SHA256SUMS"), &sums),
        image: FakeSource::new(data),
    };
    let d = dir("cache");
    let no = AtomicBool::new(false);

    let mut stages = Vec::new();
    let a = acquire(&net, &resolved(None), &d, &no, |e| {
        if let AcquireEvent::Stage { stage } = e {
            stages.push(stage)
        }
    })
    .await
    .unwrap();
    assert_eq!(stages, [Stage::Checksum, Stage::Download, Stage::Verify]);
    assert_eq!(a.assurance, Assurance::Checksum);
    assert!(!a.from_cache);
    assert!(a.path.exists());

    // Zweiter Lauf: Datei ist da und passt → kein Download.
    let a2 = acquire(&net, &resolved(None), &d, &no, |_| {})
        .await
        .unwrap();
    assert!(a2.from_cache);
    assert_eq!(net.image.opens.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn signed_sums_ok() {
    let data = image_data();
    let sums = format!("{}  test-1.0.iso\n", sha256(&data));
    let (public, fp, sign) = keypair();
    let net = Net {
        pages: FakeFetcher::new()
            .page(&format!("{BASE}SHA256SUMS"), &sums)
            .page_bytes(&format!("{BASE}SHA256SUMS.gpg"), &sign(sums.as_bytes()))
            .page_bytes(&format!("{BASE}key.asc"), &public),
        image: FakeSource::new(data),
    };
    let img = resolved(Some(sig_meta(
        &fp,
        SignedTarget::ChecksumFile,
        "SHA256SUMS.gpg",
    )));
    let a = acquire(&net, &img, &dir("signed"), &AtomicBool::new(false), |_| {})
        .await
        .unwrap();
    assert_eq!(a.assurance, Assurance::Signed);
    assert_eq!(a.signer.unwrap().fingerprint, fp);
}

#[tokio::test]
async fn tampered_sums_abort_before_download() {
    let data = image_data();
    let real = format!("{}  test-1.0.iso\n", sha256(&data));
    let evil = format!("{}  test-1.0.iso\n", "e".repeat(64));
    let (public, fp, sign) = keypair();
    let net = Net {
        pages: FakeFetcher::new()
            .page(&format!("{BASE}SHA256SUMS"), &evil) // ausgetauscht
            .page_bytes(&format!("{BASE}SHA256SUMS.gpg"), &sign(real.as_bytes()))
            .page_bytes(&format!("{BASE}key.asc"), &public),
        image: FakeSource::new(data),
    };
    let img = resolved(Some(sig_meta(
        &fp,
        SignedTarget::ChecksumFile,
        "SHA256SUMS.gpg",
    )));
    let err = acquire(
        &net,
        &img,
        &dir("tampered"),
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AcquireError::Gpg(_)), "{err}");
    assert_eq!(
        net.image.opens.load(Ordering::SeqCst),
        0,
        "darf gar nicht erst herunterladen"
    );
}

#[tokio::test]
async fn wrong_hash_deletes_file() {
    let data = image_data();
    let sums = format!("{}  test-1.0.iso\n", "0".repeat(64));
    let net = Net {
        pages: FakeFetcher::new().page(&format!("{BASE}SHA256SUMS"), &sums),
        image: FakeSource::new(data),
    };
    let d = dir("mismatch");
    let err = acquire(&net, &resolved(None), &d, &AtomicBool::new(false), |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, AcquireError::ChecksumMismatch { .. }),
        "{err}"
    );
    assert!(!d.join("test-1.0.iso").exists(), "falsche Datei muss weg");
}

#[tokio::test]
async fn image_signature_checked_and_tamper_detected() {
    let data = image_data();
    let sums = format!("{}  test-1.0.iso\n", sha256(&data));
    let (public, fp, sign) = keypair();
    let img = resolved(Some(sig_meta(&fp, SignedTarget::Image, "test-1.0.iso.sig")));

    // Gültig
    let net = Net {
        pages: FakeFetcher::new()
            .page(&format!("{BASE}SHA256SUMS"), &sums)
            .page_bytes(&format!("{BASE}test-1.0.iso.sig"), &sign(&data))
            .page_bytes(&format!("{BASE}key.asc"), &public),
        image: FakeSource::new(data.clone()),
    };
    let a = acquire(&net, &img, &dir("imgsig"), &AtomicBool::new(false), |_| {})
        .await
        .unwrap();
    assert_eq!(a.assurance, Assurance::Signed);

    // Signatur über ANDERE Daten → Image wird verworfen, obwohl die Prüfsumme passt.
    let net = Net {
        pages: FakeFetcher::new()
            .page(&format!("{BASE}SHA256SUMS"), &sums)
            .page_bytes(&format!("{BASE}test-1.0.iso.sig"), &sign(b"etwas anderes"))
            .page_bytes(&format!("{BASE}key.asc"), &public),
        image: FakeSource::new(data),
    };
    let d = dir("imgsig-bad");
    let err = acquire(&net, &img, &d, &AtomicBool::new(false), |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, AcquireError::Gpg(_)), "{err}");
    assert!(!d.join("test-1.0.iso").exists());
}

#[tokio::test]
async fn hostile_filenames_rejected() {
    for name in [
        "../../.bashrc",
        "a/b.iso",
        ".hidden.iso",
        "con:.iso",
        "x.iso\n",
        "",
    ] {
        let mut img = resolved(None);
        img.filename = name.into();
        let net = Net {
            pages: FakeFetcher::new(),
            image: FakeSource::new(vec![]),
        };
        let err = acquire(&net, &img, &dir("names"), &AtomicBool::new(false), |_| {})
            .await
            .unwrap_err();
        assert!(
            matches!(err, AcquireError::BadFilename(_)),
            "{name:?}: {err}"
        );
    }
}

#[tokio::test]
async fn file_not_listed_in_sums() {
    let net = Net {
        pages: FakeFetcher::new().page(
            &format!("{BASE}SHA256SUMS"),
            &format!("{}  other.iso\n", "a".repeat(64)),
        ),
        image: FakeSource::new(image_data()),
    };
    let err = acquire(
        &net,
        &resolved(None),
        &dir("unlisted"),
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, AcquireError::ChecksumNotListed { .. }),
        "{err}"
    );
}

// --- Hersteller ohne Prüfsumme: nur die Herkunft ist abgesichert ------------------------------

const MS_HOST: &str = "software-static.download.prss.microsoft.com";

fn origin_image() -> ResolvedImage {
    ResolvedImage {
        variant_id: "windows-server-2025".into(),
        version: "2025 (Testversion)".into(),
        build: None,
        filename: "WindowsServer2025_Eval.iso".into(),
        url: "https://go.microsoft.com/fwlink/?linkid=1".parse().unwrap(),
        size: None,
        checksum: ResolvedChecksum::HttpsOrigin {
            allowed_hosts: vec![MS_HOST.into()],
        },
        signature: None,
        compression: Compression::None,
        flash_mode: FlashMode::Windows,
    }
}

fn origin_net(final_host: &str) -> Net {
    let mut image = FakeSource::new(image_data());
    image.redirect_to = Some(
        format!("https://{final_host}/x/Server_EVAL.iso")
            .parse()
            .unwrap(),
    );
    Net {
        pages: FakeFetcher::new(),
        image,
    }
}

#[tokio::test]
async fn origin_only_download_from_allowed_server() {
    let dir = std::env::temp_dir().join(format!("sf-origin-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut stages = Vec::new();
    let a = acquire(
        &origin_net(MS_HOST),
        &origin_image(),
        &dir,
        &AtomicBool::new(false),
        |e| {
            if let AcquireEvent::Stage { stage } = e {
                stages.push(stage)
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(a.assurance, Assurance::Origin);
    assert_eq!(a.hash, sha256(&image_data()));
    assert_eq!(
        stages,
        [Stage::Download],
        "keine Prüfsummen-Schritte vortäuschen"
    );
    // Hash festgehalten …
    let side = std::fs::read_to_string(dir.join("WindowsServer2025_Eval.iso.sha256")).unwrap();
    assert_eq!(side.trim(), a.hash);

    // … zweiter Lauf aus dem Cache …
    let b = acquire(
        &origin_net(MS_HOST),
        &origin_image(),
        &dir,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap();
    assert!(b.from_cache);
    assert_eq!(b.assurance, Assurance::Origin);

    // … beschädigte Datei im Cache wird erkannt und neu geladen.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("WindowsServer2025_Eval.iso"))
        .unwrap();
    f.write_all(b"kaputt").unwrap();
    drop(f);
    let c = acquire(
        &origin_net(MS_HOST),
        &origin_image(),
        &dir,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap();
    assert!(
        !c.from_cache,
        "beschädigte Datei darf nicht verwendet werden"
    );
    assert_eq!(c.hash, sha256(&image_data()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn origin_only_refuses_foreign_server() {
    let dir = std::env::temp_dir().join(format!("sf-origin-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Weiterleitung auf einen fremden Server – auch wenn der Name ähnlich aussieht.
    for host in [
        "evil.example",
        "software-static.download.prss.microsoft.com.evil.example",
    ] {
        let err = acquire(
            &origin_net(host),
            &origin_image(),
            &dir,
            &AtomicBool::new(false),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                AcquireError::Download(
                    sealflash_core::download::DownloadError::UntrustedHost { .. }
                )
            ),
            "{host}: {err}"
        );
        let left: Vec<_> = std::fs::read_dir(&dir)
            .map(|d| d.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(
            left.iter().all(|n| !n.to_string_lossy().ends_with(".iso")
                && !n.to_string_lossy().ends_with(".part")),
            "nichts vom fremden Server gespeichert: {left:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
