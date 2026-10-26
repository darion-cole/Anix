//! The macOS build's updater: reading `mac-update.json`, ordering builds, finding the running
//! bundle and swapping it, with a loopback server as GitHub and a fake `.app` in a temp folder.

mod plugin_support;

use anix_lib::clients::mac_update::{
    bundle_target, check_at, download_and_install, install_bundle, is_newer_build, manifest_url, parse_manifest,
    previous_path, MacRelease,
};
use anix_lib::clients::update::{sha256_file, AppImageAsset};
use plugin_support::{Req, Resp, Site};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const REPO: &str = "someone/Anix";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn manifest(version: &str, build: u64) -> Value {
    json!({
        "version": version,
        "build": build,
        "url": format!("https://github.com/{}/releases/download/mac-v{}-b{}/Anix.app.tar.gz", REPO, version, build),
        "sha256": SHA,
        "size": 1234,
        "notes": "fix(macos): the icon",
        "published_at": "2026-10-26T20:00:00Z",
        "release_url": format!("https://github.com/{}/releases/tag/mac-v{}-b{}", REPO, version, build),
    })
}

fn release(version: &str, build: u64) -> MacRelease {
    parse_manifest(&manifest(version, build), REPO).unwrap()
}

/// A fresh empty folder for one test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("anix-mac-update-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `<dir>/<name>` as a minimal app bundle whose program prints `marker`.
fn fake_app(dir: &Path, name: &str, marker: &str) -> PathBuf {
    let app = dir.join(name);
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/MacOS/anix"), marker).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
    app
}

fn marker(app: &Path) -> String {
    std::fs::read_to_string(app.join("Contents/MacOS/anix")).unwrap()
}

/// Packs `<dir>/<name>` into `archive` the way the macOS workflow does.
fn pack(dir: &Path, name: &str, archive: &Path) {
    let status = std::process::Command::new("tar").arg("-czf").arg(archive).arg("-C").arg(dir).arg(name).status().unwrap();
    assert!(status.success());
}

#[test]
fn the_update_document_lives_in_the_forks_latest_release() {
    assert_eq!(
        manifest_url(REPO).as_deref(),
        Some("https://github.com/someone/Anix/releases/latest/download/mac-update.json")
    );
    for bad in ["", "someone", "someone/", "/Anix", "a/b/c", "some one/Anix", "someone/../x", "someone/An?x"] {
        assert_eq!(manifest_url(bad), None, "{:?}", bad);
    }
}

#[test]
fn a_build_is_shown_as_its_version_with_the_build_number() {
    let r = release("1.3.0", 12);
    assert_eq!((r.version.as_str(), r.build), ("1.3.0", 12));
    assert_eq!(r.info.version, "1.3.0-mac.12");
    assert_eq!(r.info.changelog, "fix(macos): the icon");
    assert_eq!(r.info.release_url, "https://github.com/someone/Anix/releases/tag/mac-v1.3.0-b12");
    let asset = r.info.appimage.unwrap();
    assert_eq!(asset.sha256, SHA);
    assert_eq!(asset.size, Some(1234));
}

#[test]
fn archives_from_anywhere_but_the_forks_releases_are_refused() {
    let with = |key: &str, value: Value| {
        let mut m = manifest("1.3.0", 2);
        m[key] = value;
        parse_manifest(&m, REPO)
    };
    for url in [
        "https://github.com/other/Anix/releases/download/x/Anix.app.tar.gz",
        "https://evil.example/someone/Anix/releases/download/x/Anix.app.tar.gz",
        "http://github.com/someone/Anix/releases/download/x/Anix.app.tar.gz",
        "https://github.com/someone/Anix/releases/download/../../other/x.tar.gz",
        "file:///tmp/Anix.app.tar.gz",
    ] {
        assert_eq!(with("url", json!(url)), None, "{}", url);
    }
    for sha in ["", "abc", &"g".repeat(64), &"0".repeat(63)] {
        assert_eq!(with("sha256", json!(sha)), None, "{:?}", sha);
    }
    assert_eq!(with("build", json!(0)), None);
    assert_eq!(with("build", json!("3")), None);
    assert_eq!(with("version", json!("1.3.0; rm -rf")), None);
    assert_eq!(with("version", json!("")), None);
    // A release page elsewhere falls back to the fork's releases.
    let page = with("release_url", json!("https://evil.example/")).unwrap();
    assert_eq!(page.info.release_url, "https://github.com/someone/Anix/releases");
    // The fork named at build time must itself be valid.
    assert_eq!(parse_manifest(&manifest("1.3.0", 2), "not a repo"), None);
}

#[test]
fn builds_are_ordered_by_version_then_build_number() {
    assert!(is_newer_build(&release("1.3.0", 5), "1.3.0", 4));
    assert!(!is_newer_build(&release("1.3.0", 4), "1.3.0", 4));
    assert!(!is_newer_build(&release("1.3.0", 3), "1.3.0", 4));
    // A new upstream version wins whatever the build numbers say.
    assert!(is_newer_build(&release("1.4.0", 1), "1.3.0", 9));
    assert!(!is_newer_build(&release("1.2.0", 50), "1.3.0", 4));
    // A build made outside the workflow (build 0) is offered the published build.
    assert!(is_newer_build(&release("1.3.0", 1), "1.3.0", 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_check_offers_only_a_newer_build() {
    let answer = Arc::new(Mutex::new(Resp::status(404)));
    let slot = answer.clone();
    let site = Site::start(move |request: &Req| match request.route() {
        "/mac-update.json" => {
            let a = slot.lock().unwrap();
            Resp { status: a.status, headers: a.headers.clone(), body: a.body.clone() }
        }
        _ => Resp::status(404),
    })
    .await;
    let url = site.url("/mac-update.json");
    let client = reqwest::Client::new();

    // No release yet.
    assert_eq!(check_at(&client, &url, REPO, "1.3.0", 4).await, Ok(None));

    *answer.lock().unwrap() = Resp::json(manifest("1.3.0", 4));
    assert_eq!(check_at(&client, &url, REPO, "1.3.0", 4).await, Ok(None));

    *answer.lock().unwrap() = Resp::json(manifest("1.3.0", 5));
    let offered = check_at(&client, &url, REPO, "1.3.0", 4).await.unwrap().unwrap();
    assert_eq!(offered.version, "1.3.0-mac.5");

    // An unusable document is an error, not "up to date".
    *answer.lock().unwrap() = Resp::json(json!({ "version": "1.3.0" }));
    assert!(check_at(&client, &url, REPO, "1.3.0", 4).await.is_err());
    *answer.lock().unwrap() = Resp::status(500);
    assert!(check_at(&client, &url, REPO, "1.3.0", 4).await.is_err());
}

#[test]
fn only_a_writable_app_bundle_can_be_replaced() {
    let dir = temp_dir("target");
    let app = fake_app(&dir, "Anix.app", "old");
    let exe = app.join("Contents/MacOS/anix");
    assert_eq!(bundle_target(&exe, 501), Ok(app.clone()));

    assert!(bundle_target(&exe, 0).unwrap_err().contains("root"));
    assert!(bundle_target(&dir.join("anix"), 501).is_err());
    assert!(bundle_target(Path::new("Anix.app/Contents/MacOS/anix"), 501).is_err());
    let translocated = dir.join("AppTranslocation/X/d/Anix.app/Contents/MacOS/anix");
    assert!(bundle_target(&translocated, 501).unwrap_err().contains("Applications"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn installing_swaps_the_bundle_and_keeps_the_previous_one() {
    let dir = temp_dir("install");
    let apps = dir.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let installed = fake_app(&apps, "Anix.app", "old");
    let build = dir.join("build");
    std::fs::create_dir_all(&build).unwrap();
    fake_app(&build, "Anix.app", "new");
    let archive = apps.join(".Anix.app.update.tar.gz");
    pack(&build, "Anix.app", &archive);
    let sha = sha256_file(&archive).unwrap();

    let previous = install_bundle(&archive, &installed, &sha).unwrap();
    assert_eq!(previous, previous_path(&installed));
    assert_eq!(marker(&installed), "new");
    assert_eq!(marker(&previous), "old");
    assert!(!archive.exists());
    assert!(!apps.join(".Anix.app.update").exists());

    // A second update replaces the kept copy.
    fake_app(&build, "Anix.app", "newer");
    pack(&build, "Anix.app", &archive);
    let sha = sha256_file(&archive).unwrap();
    install_bundle(&archive, &installed, &sha).unwrap();
    assert_eq!((marker(&installed).as_str(), marker(&previous).as_str()), ("newer", "new"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_renamed_app_keeps_its_name_after_an_update() {
    let dir = temp_dir("renamed");
    let installed = fake_app(&dir, "My Anix.app", "old");
    let build = dir.join("build");
    std::fs::create_dir_all(&build).unwrap();
    fake_app(&build, "Anix.app", "new");
    let archive = dir.join("update.tar.gz");
    pack(&build, "Anix.app", &archive);
    let sha = sha256_file(&archive).unwrap();
    install_bundle(&archive, &installed, &sha).unwrap();
    assert_eq!(marker(&installed), "new");
    assert!(!dir.join("Anix.app").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_download_leaves_the_app_as_it_was() {
    let dir = temp_dir("refuse");
    let installed = fake_app(&dir, "Anix.app", "old");
    let build = dir.join("build");
    std::fs::create_dir_all(&build).unwrap();
    fake_app(&build, "Anix.app", "new");

    // Wrong checksum.
    let archive = dir.join("update.tar.gz");
    pack(&build, "Anix.app", &archive);
    let err = install_bundle(&archive, &installed, SHA).unwrap_err();
    assert!(err.contains("checksum"), "{}", err);
    assert_eq!(marker(&installed), "old");
    assert!(!archive.exists());

    // An archive without an app in it.
    std::fs::write(build.join("readme.txt"), "x").unwrap();
    pack(&build, "readme.txt", &archive);
    let sha = sha256_file(&archive).unwrap();
    assert!(install_bundle(&archive, &installed, &sha).is_err());
    assert_eq!(marker(&installed), "old");

    // Not an archive at all.
    std::fs::write(&archive, "not gzip").unwrap();
    let sha = sha256_file(&archive).unwrap();
    assert!(install_bundle(&archive, &installed, &sha).is_err());
    assert_eq!(marker(&installed), "old");
    assert!(!previous_path(&installed).exists());
    assert!(!dir.join(".Anix.app.update").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_archive_is_downloaded_with_progress_and_installed() {
    let dir = temp_dir("download");
    let installed = fake_app(&dir, "Anix.app", "old");
    let build = dir.join("build");
    std::fs::create_dir_all(&build).unwrap();
    fake_app(&build, "Anix.app", "new");
    let packed = dir.join("packed.tar.gz");
    pack(&build, "Anix.app", &packed);
    let bytes = std::fs::read(&packed).unwrap();
    let sha = sha256_file(&packed).unwrap();

    let body = bytes.clone();
    let site = Site::start(move |request: &Req| match request.route() {
        "/Anix.app.tar.gz" => Resp::new(200, "application/gzip", body.clone()),
        _ => Resp::status(404),
    })
    .await;
    let asset = AppImageAsset { url: site.url("/Anix.app.tar.gz"), sha256: sha, size: Some(bytes.len() as u64) };
    let mut seen = Vec::new();
    download_and_install(&reqwest::Client::new(), &asset, &installed, |f| seen.push(f)).await.unwrap();
    assert_eq!(marker(&installed), "new");
    assert_eq!(seen.last().copied(), Some(1.0));

    // A missing archive changes nothing.
    let gone = AppImageAsset { url: site.url("/missing"), ..asset };
    assert!(download_and_install(&reqwest::Client::new(), &gone, &installed, |_| {}).await.is_err());
    assert_eq!(marker(&installed), "new");
    let _ = std::fs::remove_dir_all(&dir);
}
