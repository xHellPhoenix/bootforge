//! Tests der Fallback-Kette Online → Cache → eingebaut.

use sealflash_core::catalog::{
    self,
    load::{Origin, load},
};
use sealflash_core::http::fake::FakeFetcher;
use url::Url;

const URL: &str = "https://raw.githubusercontent.com/x/y/main/catalog.json";

fn url() -> Url {
    URL.parse().unwrap()
}

fn tempdir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sealflash-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Gültiger Katalog, der sich vom eingebauten unterscheidet (am Datum erkennbar).
fn remote_catalog() -> String {
    catalog::BUNDLED_CATALOG_JSON.replacen(
        r#""updated": "2026-11-01""#,
        r#""updated": "2099-01-01""#,
        1,
    )
}

#[tokio::test]
async fn remote_ok_is_used_and_cached() {
    let dir = tempdir("remote-ok");
    let f = FakeFetcher::new().page_with_etag(URL, &remote_catalog(), "\"v1\"");
    let l = load(&f, &url(), &dir).await;
    assert_eq!(l.origin, Origin::Remote);
    assert_eq!(l.catalog.updated, "2099-01-01");
    assert!(dir.join("catalog.json").exists());

    // Zweiter Start: Server sagt 304 → Cache wird genutzt, gilt aber als aktuell.
    let l2 = load(&f, &url(), &dir).await;
    assert_eq!(l2.origin, Origin::Remote);
    assert_eq!(l2.catalog.updated, "2099-01-01");
}

#[tokio::test]
async fn offline_uses_cache_then_bundled() {
    let dir = tempdir("offline");
    // Ohne Cache → eingebaut
    let l = load(&FakeFetcher::offline(), &url(), &dir).await;
    assert_eq!(l.origin, Origin::Bundled);
    assert!(!l.remote_rejected, "offline ist nicht „abgelehnt“");
    assert!(l.fallback_reason.unwrap().contains("nicht erreichbar"));

    // Cache befüllen, dann offline → Cache
    load(
        &FakeFetcher::new().page(URL, &remote_catalog()),
        &url(),
        &dir,
    )
    .await;
    let l = load(&FakeFetcher::offline(), &url(), &dir).await;
    assert_eq!(l.origin, Origin::Cache);
    assert_eq!(l.catalog.updated, "2099-01-01");
}

#[tokio::test]
async fn invalid_remote_is_rejected_and_cache_kept() {
    let dir = tempdir("invalid");
    load(
        &FakeFetcher::new().page(URL, &remote_catalog()),
        &url(),
        &dir,
    )
    .await;

    let evil = remote_catalog().replace(
        "https://releases.ubuntu.com/",
        "http://releases.ubuntu.com/",
    );
    let l = load(&FakeFetcher::new().page(URL, &evil), &url(), &dir).await;
    assert_eq!(l.origin, Origin::Cache);
    assert!(l.remote_rejected);
    assert!(l.fallback_reason.unwrap().contains("abgelehnt"));
    // Der kaputte Katalog darf den guten Cache NICHT überschrieben haben:
    let cached = std::fs::read_to_string(dir.join("catalog.json")).unwrap();
    assert!(!cached.contains("http://releases.ubuntu.com/"));
}

#[tokio::test]
async fn corrupt_cache_falls_back_to_bundled() {
    let dir = tempdir("corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("catalog.json"), "{ halb geschrieben").unwrap();
    let l = load(&FakeFetcher::offline(), &url(), &dir).await;
    assert_eq!(l.origin, Origin::Bundled);
}
