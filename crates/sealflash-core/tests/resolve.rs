//! Tests der Versionsermittlung mit nachgebauten Server-Antworten (kein Internet nötig).
//! Die Fixtures bilden echte Eigenheiten der Server nach (Stand 01.11.2026).

use sealflash_core::catalog::{self, Catalog, HashAlgorithm, Variant};
use sealflash_core::http::fake::FakeFetcher;
use sealflash_core::resolve::{ResolveError, ResolvedChecksum, resolve};

fn catalog() -> Catalog {
    catalog::bundled().unwrap().0
}

fn variant(id: &str) -> Variant {
    catalog()
        .distros
        .into_iter()
        .flat_map(|d| d.variants)
        .find(|v| v.id == id)
        .unwrap()
}

/// Minimal-Listing im Apache-Stil.
fn listing(links: &[&str]) -> String {
    let mut s = String::from(r#"<html><body><a href="?C=N;O=D">Name</a><a href="../">Parent</a>"#);
    for l in links {
        s.push_str(&format!(r#"<a href="{l}">{l}</a>"#));
    }
    s
}

#[tokio::test]
async fn ubuntu_lts_picks_newest_even_lts_and_point_release() {
    let f = FakeFetcher::new()
        .page(
            "https://releases.ubuntu.com/",
            &listing(&[
                "22.04/", "24.04/", "24.04.3/", "25.10/", "26.04/", "26.10/", "noble/", "streams/",
            ]),
        )
        .page(
            "https://releases.ubuntu.com/26.04/",
            &listing(&[
                "ubuntu-26.04-desktop-amd64.iso",
                "ubuntu-26.04.1-desktop-amd64.iso",
                "ubuntu-26.04.1-live-server-amd64.iso",
                "SHA256SUMS",
                "SHA256SUMS.gpg",
            ]),
        )
        .size(
            "https://releases.ubuntu.com/26.04/ubuntu-26.04.1-desktop-amd64.iso",
            6_482_409_472,
        );

    let r = resolve(&f, &variant("ubuntu-desktop-lts")).await.unwrap();
    assert_eq!(r.version, "26.04.1");
    assert_eq!(
        r.url.as_str(),
        "https://releases.ubuntu.com/26.04/ubuntu-26.04.1-desktop-amd64.iso"
    );
    assert_eq!(r.size, Some(6_482_409_472));
    assert_eq!(
        r.checksum,
        ResolvedChecksum::File {
            url: "https://releases.ubuntu.com/26.04/SHA256SUMS"
                .parse()
                .unwrap(),
            algorithm: HashAlgorithm::Sha256
        }
    );
    let sig = r.signature.unwrap();
    assert_eq!(
        sig.url.unwrap().as_str(),
        "https://releases.ubuntu.com/26.04/SHA256SUMS.gpg"
    );
}

#[tokio::test]
async fn ubuntu_latest_includes_interim_release() {
    let f = FakeFetcher::new()
        .page(
            "https://releases.ubuntu.com/",
            &listing(&["24.04/", "26.04/", "26.10/"]),
        )
        .page(
            "https://releases.ubuntu.com/26.10/",
            &listing(&["ubuntu-26.10-desktop-amd64.iso", "SHA256SUMS"]),
        );
    let r = resolve(&f, &variant("ubuntu-desktop-latest"))
        .await
        .unwrap();
    assert_eq!(r.version, "26.10");
}

#[tokio::test]
async fn fedora_skips_empty_new_release_dir_and_finds_checksum_by_regex() {
    let f = FakeFetcher::new()
        .page(
            "https://dl.fedoraproject.org/pub/fedora/linux/releases/",
            &listing(&["9/", "43/", "44/", "45/", "test/"]),
        )
        // 45 existiert schon, ist aber noch leer → Sackgasse
        .page(
            "https://dl.fedoraproject.org/pub/fedora/linux/releases/45/Workstation/x86_64/iso/",
            &listing(&[]),
        )
        .page(
            "https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Workstation/x86_64/iso/",
            &listing(&[
                "Fedora-Workstation-44-1.7-x86_64-CHECKSUM",
                "Fedora-Workstation-Live-44-1.7.x86_64.iso",
            ]),
        );

    let r = resolve(&f, &variant("fedora-workstation")).await.unwrap();
    assert_eq!(r.version, "44");
    assert_eq!(r.filename, "Fedora-Workstation-Live-44-1.7.x86_64.iso");
    let ResolvedChecksum::File { url, .. } = &r.checksum else {
        panic!()
    };
    assert!(
        url.as_str()
            .ends_with("/44/Workstation/x86_64/iso/Fedora-Workstation-44-1.7-x86_64-CHECKSUM")
    );
    assert!(
        r.signature.unwrap().url.is_none(),
        "Clearsign hat keine eigene Signaturdatei"
    );
}

#[tokio::test]
async fn backtracking_is_limited() {
    // Alle Versionen leer → nach MAX_BACKTRACK (3) Versuchen aufgeben, nicht alle 10 abklappern.
    let dirs: Vec<String> = (35..45).map(|n| format!("{n}/")).collect();
    let refs: Vec<&str> = dirs.iter().map(String::as_str).collect();
    let mut f = FakeFetcher::new().page(
        "https://dl.fedoraproject.org/pub/fedora/linux/releases/",
        &listing(&refs),
    );
    for n in 35..45 {
        f = f.page(
            &format!(
                "https://dl.fedoraproject.org/pub/fedora/linux/releases/{n}/Workstation/x86_64/iso/"
            ),
            "",
        );
    }
    let err = resolve(&f, &variant("fedora-workstation"))
        .await
        .unwrap_err();
    assert!(matches!(err, ResolveError::NoMatch { .. }), "{err}");
    assert_eq!(f.requests.lock().unwrap().len(), 1 + 3);
}

#[tokio::test]
async fn leap_ignores_old_numbering() {
    let f = FakeFetcher::new()
        .page(
            "https://download.opensuse.org/distribution/leap/",
            &listing(&["./15.6/", "./16.0/", "./16.1/", "./42.3/"]),
        )
        .page(
            "https://download.opensuse.org/distribution/leap/16.1/offline/",
            &listing(&[
                "./Leap-16.1-offline-installer-x86_64-Build60.2.install.iso",
                "./Leap-16.1-offline-installer-x86_64.install.iso",
                "./Leap-16.1-offline-installer-x86_64.install.iso.sha512",
                "./Leap-16.1-offline-installer-x86_64.install.iso.mirrorlist",
            ]),
        );
    let r = resolve(&f, &variant("opensuse-leap")).await.unwrap();
    assert_eq!(r.version, "16.1");
    assert_eq!(r.filename, "Leap-16.1-offline-installer-x86_64.install.iso");
    let ResolvedChecksum::File { url, algorithm } = &r.checksum else {
        panic!()
    };
    assert_eq!(*algorithm, HashAlgorithm::Sha512);
    assert!(
        url.as_str()
            .ends_with("/16.1/offline/Leap-16.1-offline-installer-x86_64.install.iso.sha512")
    );
}

#[tokio::test]
async fn truenas_flat_listing_skips_betas() {
    let f = FakeFetcher::new().page(
        "https://download.truenas.com/",
        &listing(&[
            "TrueNAS-SCALE-Fangtooth/25.04.2.4/TrueNAS-SCALE-25.04.2.4.iso",
            "TrueNAS-SCALE-Goldeye/25.10.7/TrueNAS-SCALE-25.10.7.iso",
            "TrueNAS-SCALE-Goldeye/25.10.7/TrueNAS-SCALE-25.10.7.iso.sha256",
            "TrueNAS-SCALE-Goldeye/25.10.10/TrueNAS-SCALE-25.10.10.iso",
            "TrueNAS-26-BETA/26.0.0-BETA.3/TrueNAS-26.0.0-BETA.3.iso",
        ]),
    );
    let r = resolve(&f, &variant("truenas")).await.unwrap();
    assert_eq!(
        r.version, "25.10.10",
        "numerisch, nicht alphabetisch; ohne Beta"
    );
    assert_eq!(
        r.url.as_str(),
        "https://download.truenas.com/TrueNAS-SCALE-Goldeye/25.10.10/TrueNAS-SCALE-25.10.10.iso"
    );
    let ResolvedChecksum::File { url, .. } = &r.checksum else {
        panic!()
    };
    assert_eq!(
        url.as_str(),
        "https://download.truenas.com/TrueNAS-SCALE-Goldeye/25.10.10/TrueNAS-SCALE-25.10.10.iso.sha256"
    );
}

#[tokio::test]
async fn popos_json_api() {
    let f = FakeFetcher::new().page(
        "https://api.pop-os.org/builds/24.04/intel",
        r#"{"version":"24.04","url":"https://iso.pop-os.org/24.04/amd64/intel/20/pop-os_24.04_amd64_intel_20.iso",
            "size":2955067392,"sha_sum":"A0EF3842AB710DB4F4407CF3499560B59DDDBBCD59BEE17BEAB7B0E99DC22B4C","build":"20"}"#,
    );
    let r = resolve(&f, &variant("popos-intel")).await.unwrap();
    assert_eq!(
        (r.version.as_str(), r.build.as_deref()),
        ("24.04", Some("20"))
    );
    assert_eq!(r.filename, "pop-os_24.04_amd64_intel_20.iso");
    assert_eq!(r.size, Some(2_955_067_392));
    assert_eq!(
        r.checksum,
        ResolvedChecksum::Value {
            value: "a0ef3842ab710db4f4407cf3499560b59dddbbcd59bee17beab7b0e99dc22b4c".into(),
            algorithm: HashAlgorithm::Sha256
        }
    );
}

#[tokio::test]
async fn json_api_rejects_http_download_url() {
    let f = FakeFetcher::new().page(
        "https://api.pop-os.org/builds/24.04/intel",
        r#"{"version":"24.04","url":"http://evil.example/pop.iso","sha_sum":"a0ef3842ab710db4f4407cf3499560b59dddbbcd59bee17beab7b0e99dc22b4c"}"#,
    );
    let err = resolve(&f, &variant("popos-intel")).await.unwrap_err();
    assert!(err.to_string().contains("nur HTTPS"), "{err}");
}

#[tokio::test]
async fn json_api_rejects_malformed_checksum() {
    let f = FakeFetcher::new().page(
        "https://api.pop-os.org/builds/24.04/intel",
        r#"{"version":"24.04","url":"https://iso.pop-os.org/x.iso","sha_sum":"kaputt"}"#,
    );
    assert!(matches!(
        resolve(&f, &variant("popos-intel")).await.unwrap_err(),
        ResolveError::BadChecksumValue { .. }
    ));
}

#[tokio::test]
async fn arch_signature_is_relative_to_image() {
    let f = FakeFetcher::new().page(
        "https://geo.mirror.pkgbuild.com/iso/latest/",
        &listing(&[
            "archlinux-2026.11.01-x86_64.iso",
            "archlinux-2026.11.01-x86_64.iso.sig",
            "archlinux-bootstrap-2026.11.01-x86_64.tar.zst",
            "archlinux-x86_64.iso",
            "sha256sums.txt",
        ]),
    );
    let r = resolve(&f, &variant("archlinux")).await.unwrap();
    assert_eq!(r.version, "2026.11.01");
    assert_eq!(
        r.signature.unwrap().url.unwrap().as_str(),
        "https://geo.mirror.pkgbuild.com/iso/latest/archlinux-2026.11.01-x86_64.iso.sig"
    );
}

#[tokio::test]
async fn missing_listing_is_a_clear_error() {
    let f = FakeFetcher::new();
    let err = resolve(&f, &variant("debian-netinst")).await.unwrap_err();
    assert!(err.to_string().contains("HTTP 404"), "{err}");
}

#[tokio::test]
async fn windows_is_downloaded_manually() {
    // Microsoft erlaubt keine automatisierten Downloads → nichts zu ermitteln, kein Netzzugriff.
    let f = FakeFetcher::new();
    let err = resolve(&f, &variant("windows-11")).await.unwrap_err();
    assert!(matches!(err, ResolveError::Manual));
    assert!(f.requests.lock().unwrap().is_empty());
}
