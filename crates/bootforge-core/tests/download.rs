//! Download-Tests mit einer Fake-Quelle: Fortsetzen, Abbruch, Server-Eigenheiten.

use std::sync::atomic::{AtomicBool, Ordering};

use bootforge_core::catalog::HashAlgorithm;
use bootforge_core::download::fake::FakeSource;
use bootforge_core::download::{DownloadError, download};
use bootforge_core::verify::sums::Hasher;
use url::Url;

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

fn sha256(d: &[u8]) -> String {
    let mut h = Hasher::new(HashAlgorithm::Sha256);
    h.update(d);
    h.finish_hex()
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bf-dl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("image.iso")
}

fn url() -> Url {
    "https://example.org/image.iso".parse().unwrap()
}

#[tokio::test]
async fn full_download_hashes_and_renames() {
    let d = data(50_000);
    let src = FakeSource::new(d.clone());
    let dest = tmp("full");
    let mut last = None;
    let r = download(
        &src,
        &url(),
        &dest,
        HashAlgorithm::Sha256,
        &AtomicBool::new(false),
        |p| last = Some(p),
    )
    .await
    .unwrap();
    assert_eq!(r.size, 50_000);
    assert_eq!(r.hash, sha256(&d));
    assert_eq!(std::fs::read(&dest).unwrap(), d);
    assert!(
        !dest.with_file_name("image.iso.part").exists(),
        "Teil-Datei aufgeräumt"
    );
    assert_eq!(last.unwrap().done, 50_000);
}

#[tokio::test]
async fn resumes_after_connection_loss() {
    let d = data(50_000);
    let src = FakeSource::new(d.clone());
    *src.fail_after.lock().unwrap() = Some(20_000);
    let dest = tmp("resume");
    let no = AtomicBool::new(false);

    let err = download(&src, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {})
        .await
        .unwrap_err();
    assert!(matches!(err, DownloadError::Fetch(_)), "{err}");
    assert!(!dest.exists());

    // Zweiter Versuch: setzt bei 20 000 fort, Hash stimmt trotzdem über die ganze Datei.
    let r = download(&src, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {})
        .await
        .unwrap();
    assert_eq!(*src.last_from.lock().unwrap(), 20_000);
    assert_eq!(r.hash, sha256(&d));
    assert_eq!(std::fs::read(&dest).unwrap(), d);
}

#[tokio::test]
async fn restarts_when_server_ignores_range() {
    let d = data(30_000);
    let mut src = FakeSource::new(d.clone());
    src.supports_range = false;
    *src.fail_after.lock().unwrap() = Some(10_000);
    let dest = tmp("norange");
    let no = AtomicBool::new(false);
    let _ = download(&src, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {}).await;
    let r = download(&src, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {})
        .await
        .unwrap();
    assert_eq!(r.hash, sha256(&d), "kein doppelter Anfang in der Datei");
    assert_eq!(std::fs::read(&dest).unwrap().len(), 30_000);
}

#[tokio::test]
async fn restarts_when_file_changed_on_server() {
    let dest = tmp("changed");
    let no = AtomicBool::new(false);
    let old = FakeSource::new(data(30_000));
    *old.fail_after.lock().unwrap() = Some(10_000);
    let _ = download(&old, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {}).await;

    // Gleiche URL, aber neuer Inhalt mit neuem ETag → If-Range greift nicht → komplett neu.
    let new_data: Vec<u8> = data(30_000).into_iter().map(|b| b ^ 0xff).collect();
    let mut new = FakeSource::new(new_data.clone());
    new.etag = "\"v2\"".into();
    let r = download(&new, &url(), &dest, HashAlgorithm::Sha256, &no, |_| {})
        .await
        .unwrap();
    assert_eq!(r.hash, sha256(&new_data));
}

#[tokio::test]
async fn cancel_during_download_keeps_part() {
    let d = data(50_000);
    let src = FakeSource::new(d.clone());
    // 1. Versuch bricht nach 20 000 Bytes ab (Netz weg) → Teil-Datei liegt da.
    *src.fail_after.lock().unwrap() = Some(20_000);
    let dest = tmp("cancel");
    let cancel = AtomicBool::new(false);
    let _ = download(&src, &url(), &dest, HashAlgorithm::Sha256, &cancel, |_| {}).await;

    // 2. Versuch: Nutzer bricht sofort ab (erste Fortschrittsmeldung) → Teil-Datei bleibt erhalten.
    let err = download(&src, &url(), &dest, HashAlgorithm::Sha256, &cancel, |_| {
        cancel.store(true, Ordering::Relaxed)
    })
    .await
    .unwrap_err();
    assert!(matches!(err, DownloadError::Cancelled), "{err}");
    let part = dest.with_file_name("image.iso.part");
    assert_eq!(std::fs::metadata(&part).unwrap().len(), 20_000);

    // 3. Versuch: setzt wieder bei 20 000 fort.
    cancel.store(false, Ordering::Relaxed);
    let r = download(&src, &url(), &dest, HashAlgorithm::Sha256, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(*src.last_from.lock().unwrap(), 20_000);
    assert_eq!(r.hash, sha256(&d));
}

#[tokio::test]
async fn cancel_before_start() {
    let src = FakeSource::new(data(10_000));
    let dest = tmp("cancel0");
    let err = download(
        &src,
        &url(),
        &dest,
        HashAlgorithm::Sha256,
        &AtomicBool::new(true),
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, DownloadError::Cancelled));
    assert!(!dest.exists());
}
