//! Eigene Images: Prüfung gegen eingegebene Prüfsumme bzw. Hersteller-Seite, Windows-Erkennung.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use bootforge_core::acquire::{AcquireError, Assurance};
use bootforge_core::catalog::HashAlgorithm;
use bootforge_core::http::fake::FakeFetcher;
use bootforge_core::local::{Expected, is_windows_installer, verify_local};
use bootforge_core::verify::sums::Hasher;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn sha256_of(path: &PathBuf) -> String {
    let mut h = Hasher::new(HashAlgorithm::Sha256);
    h.update(&std::fs::read(path).unwrap());
    h.finish_hex()
}

const PAGE: &str = "https://www.microsoft.com/en-us/software-download/windows11";

#[test]
fn detects_windows_installer_iso() {
    assert!(is_windows_installer(&fixture("windows-mini.iso")).unwrap());
    assert!(!is_windows_installer(&fixture("linux-mini.iso")).unwrap());
    // Kein ISO9660 → kein Windows-ISO (z. B. .img oder Prüfsummendatei)
    assert!(!is_windows_installer(&fixture("ubuntu-SHA256SUMS")).unwrap());
}

#[tokio::test]
async fn user_hash_matches_and_mismatches() {
    let iso = fixture("linux-mini.iso");
    let good = Expected::Hash {
        algorithm: HashAlgorithm::Sha256,
        value: sha256_of(&iso).to_uppercase(),
    };
    let a = verify_local(
        &FakeFetcher::new(),
        &iso,
        &good,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(a.assurance, Assurance::Checksum);
    assert_eq!(a.filename, "linux-mini.iso");

    let bad = Expected::Hash {
        algorithm: HashAlgorithm::Sha256,
        value: "0".repeat(64),
    };
    let err = verify_local(
        &FakeFetcher::new(),
        &iso,
        &bad,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, AcquireError::ChecksumMismatch { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn hash_must_be_listed_on_vendor_page() {
    let iso = fixture("windows-mini.iso");
    let page = format!(
        "<table><tr><td>German 64-bit</td><td>{}</td></tr><tr><td>English 64-bit</td><td>{}</td></tr></table>",
        "1".repeat(64),
        sha256_of(&iso).to_uppercase()
    );
    let f = FakeFetcher::new().page(PAGE, &page);
    let exp = Expected::PageListing {
        url: PAGE.parse().unwrap(),
        algorithm: HashAlgorithm::Sha256,
    };
    verify_local(&f, &iso, &exp, &AtomicBool::new(false), |_| {})
        .await
        .unwrap();

    // Andere Datei (nicht in der Liste) → abgelehnt
    let other = fixture("linux-mini.iso");
    let err = verify_local(&f, &other, &exp, &AtomicBool::new(false), |_| {})
        .await
        .unwrap_err();
    assert!(
        matches!(err, AcquireError::ChecksumNotListed { .. }),
        "{err}"
    );
}

#[tokio::test]
async fn page_without_hashes_is_an_error_not_a_pass() {
    let f = FakeFetcher::new().page(PAGE, "<html>Seite umgebaut, keine Tabelle mehr</html>");
    let exp = Expected::PageListing {
        url: PAGE.parse().unwrap(),
        algorithm: HashAlgorithm::Sha256,
    };
    let err = verify_local(
        &f,
        &fixture("windows-mini.iso"),
        &exp,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, AcquireError::ChecksumNotListed { .. }));
}
