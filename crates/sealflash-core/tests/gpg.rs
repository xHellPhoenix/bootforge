//! GPG-Prüfung mit echten Signaturen der Distros (Fixtures vom 01.11.2026)
//! und mit Angriffsszenarien (manipulierte Daten, falscher Schlüssel).

use sealflash_core::catalog::HashAlgorithm;
use sealflash_core::verify::gpg::{GpgError, pinned_certs, verify_cleartext, verify_detached};
use sealflash_core::verify::sums::find_hash;

const UBUNTU_FP: &str = "843938DF228D22F7B3742BC0D94AA3F0EFE21092";
const DEBIAN_FP: &str = "DF9B9C49EAA9298432589D76DA87E80D6294BE9B";
const SUSE_FP: &str = "AD485664E901B867051AB15F35A2F86E29B700A4";
const FEDORA_FPS: [&str; 3] = [
    "C6E7F081CF80E13146676E88829B606631645531",
    "36F612DCF27F7D1A48A835E4DBFCF71C6D9F90A6",
    "4F50A6114CD5C6976A7F1179655A4B02F577861E",
];

fn fx(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn fps(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ubuntu_detached_signature_over_sums() {
    let pins = fps(&[UBUNTU_FP]);
    let certs = pinned_certs(&fx("ubuntu-key.asc"), &pins).unwrap();
    let signer = verify_detached(
        &fx("ubuntu-SHA256SUMS")[..],
        &fx("ubuntu-SHA256SUMS.gpg"),
        &certs,
        &pins,
        false,
    )
    .unwrap();
    assert_eq!(signer.fingerprint, UBUNTU_FP);
    assert!(signer.user_id.unwrap().contains("Ubuntu CD Image"));
}

#[test]
fn debian_detached_signature_over_sums() {
    let pins = fps(&[DEBIAN_FP]);
    let certs = pinned_certs(&fx("debian-key.asc"), &pins).unwrap();
    verify_detached(
        &fx("debian-SHA256SUMS")[..],
        &fx("debian-SHA256SUMS.sign"),
        &certs,
        &pins,
        false,
    )
    .unwrap();
}

#[test]
fn opensuse_v3_signature() {
    // openSUSE signiert noch mit alten v3-Signaturpaketen: standardmäßig abgelehnt …
    let pins = fps(&[SUSE_FP]);
    let certs = pinned_certs(&fx("suse-key.asc"), &pins).unwrap();
    let (data, sig) = (
        fx("suse-Current.iso.sha256"),
        fx("suse-Current.iso.sha256.asc"),
    );
    let err = verify_detached(&data[..], &sig, &certs, &pins, false).unwrap_err();
    assert!(err.to_string().contains("v3"), "{err}");
    // … nur mit ausdrücklicher Erlaubnis (`allow_v3` im Katalog) akzeptiert.
    verify_detached(&data[..], &sig, &certs, &pins, true).unwrap();
    // Und auch dann gilt: manipulierte Daten fliegen raus.
    let mut evil = data.clone();
    evil[0] ^= 1;
    assert!(verify_detached(&evil[..], &sig, &certs, &pins, true).is_err());
}

#[test]
fn fedora_clearsigned_returns_only_signed_text() {
    let pins = fps(&FEDORA_FPS);
    let certs = pinned_certs(&fx("fedora.gpg"), &pins).unwrap();
    let (text, signer) = verify_cleartext(&fx("fedora-CHECKSUM"), &certs, &pins, false).unwrap();
    assert_eq!(signer.fingerprint, FEDORA_FPS[1], "Fedora 44 Schlüssel");
    assert!(!text.contains("BEGIN PGP"));
    assert_eq!(
        find_hash(
            &text,
            "Fedora-Workstation-Live-44-1.7.x86_64.iso",
            HashAlgorithm::Sha256
        )
        .as_deref(),
        Some("1620295f6a00c27c3208f0c00b8ece4eab1ec69b9002152d97488bf26a426ddf")
    );
}

// --- Angriffe ---------------------------------------------------------------------------------

#[test]
fn tampered_sums_file_is_rejected() {
    let pins = fps(&[UBUNTU_FP]);
    let certs = pinned_certs(&fx("ubuntu-key.asc"), &pins).unwrap();
    let mut sums = fx("ubuntu-SHA256SUMS");
    sums[0] = if sums[0] == b'0' { b'1' } else { b'0' }; // ein einziges Zeichen ändern
    let err = verify_detached(
        &sums[..],
        &fx("ubuntu-SHA256SUMS.gpg"),
        &certs,
        &pins,
        false,
    )
    .unwrap_err();
    assert!(matches!(err, GpgError::BadSignature(_)), "{err}");
}

#[test]
fn tampered_clearsigned_text_is_rejected() {
    let pins = fps(&FEDORA_FPS);
    let certs = pinned_certs(&fx("fedora.gpg"), &pins).unwrap();
    let evil = String::from_utf8(fx("fedora-CHECKSUM"))
        .unwrap()
        .replace("1620295f", "deadbeef");
    assert!(verify_cleartext(evil.as_bytes(), &certs, &pins, false).is_err());
}

#[test]
fn signature_from_wrong_vendor_is_rejected() {
    // Debians Signatur, aber im Katalog ist nur Ubuntus Schlüssel erlaubt.
    let ubuntu = fps(&[UBUNTU_FP]);
    let certs = pinned_certs(&fx("ubuntu-key.asc"), &ubuntu).unwrap();
    assert!(
        verify_detached(
            &fx("debian-SHA256SUMS")[..],
            &fx("debian-SHA256SUMS.sign"),
            &certs,
            &ubuntu,
            false
        )
        .is_err()
    );
}

#[test]
fn keyring_without_pinned_key_is_rejected() {
    // Angreifer liefert einen „Schlüsselbund“ mit einem anderen Schlüssel.
    let err = pinned_certs(&fx("debian-key.asc"), &fps(&[UBUNTU_FP])).unwrap_err();
    assert!(matches!(err, GpgError::NoPinnedKey(_)));
}

#[test]
fn fedora_keyring_is_filtered_to_pins() {
    // fedora.gpg enthält viele Schlüssel; nur der für Fedora 44 bleibt übrig.
    let certs = pinned_certs(&fx("fedora.gpg"), &fps(&[FEDORA_FPS[1]])).unwrap();
    assert_eq!(certs.len(), 1);
}
