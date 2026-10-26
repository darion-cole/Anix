//! Updates for the macOS build, which is made from a fork's `macos` branch rather than released
//! by the project. Each build is published as a GitHub release of that fork with two files: the
//! app as `<name>.app.tar.gz` and `mac-update.json`, which names the version, the build number,
//! the archive's address and its SHA-256. The newest release's `mac-update.json` is read from
//! `https://github.com/<repo>/releases/latest/download/mac-update.json`.
//!
//! The fork (`<repo>`) and the build number are baked in at compile time by the macOS workflow
//! (`ANIX_MAC_UPDATE_REPO`, `ANIX_MAC_BUILD`); a build without them has no in-app updates.
//! Builds are ordered by the upstream version first and the build number second, so a rebuild
//! of the same version (a macOS fix) is still offered as an update.
//!
//! Installing replaces the whole `.app` bundle: the archive is checked against its SHA-256,
//! unpacked next to the running app, and swapped in with two renames. The previous bundle is
//! kept, hidden, as `.<name>.app.previous` beside it.
//!
//! Everything here is plain logic with no macOS calls, so it is compiled and tested on Linux
//! too; only `commands/updates.rs` uses it on macOS.

use crate::clients::http::APP_USER_AGENT;
use crate::clients::update::{is_newer_version, sha256_file, AppImageAsset, UpdateInfo};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `owner/name` of the fork that publishes macOS builds, when this build has one.
pub const UPDATE_REPO: Option<&str> = option_env!("ANIX_MAC_UPDATE_REPO");

/// This build's number from the macOS workflow; 0 for a build made elsewhere.
pub fn current_build() -> u64 {
    option_env!("ANIX_MAC_BUILD").and_then(|b| b.trim().parse().ok()).unwrap_or(0)
}

/// Name of the update document attached to every macOS release.
pub const MANIFEST_NAME: &str = "mac-update.json";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
/// The packed app is far smaller than this; a bigger download is refused.
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// A usable `owner/name`: two non-empty parts of letters, digits, `-`, `_` and `.`.
fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |p: Option<&str>| {
        p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)))
    };
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

/// Where the newest release's update document is downloaded from.
pub fn manifest_url(repo: &str) -> Option<String> {
    valid_repo(repo).then(|| format!("https://github.com/{}/releases/latest/download/{}", repo, MANIFEST_NAME))
}

/// The prefix every archive address must have: a release download of `repo` on GitHub.
fn download_prefix(repo: &str) -> String {
    format!("https://github.com/{}/releases/download/", repo)
}

/// A macOS build as `mac-update.json` describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct MacRelease {
    /// The upstream version the build was made from, like `1.3.0`.
    pub version: String,
    pub build: u64,
    /// What the update dialog shows. Its `version` reads `1.3.0-mac.12`, and its `appimage`
    /// field (the dialog's "can be installed" asset) holds the archive.
    pub info: UpdateInfo,
}

/// Reads `{ "version", "build", "url", "sha256", "size"?, "notes"?, "published_at"?,
/// "release_url"? }`. The archive must be a release download of `repo` and the checksum 64 hex
/// digits; anything else is not an update this app will install.
pub fn parse_manifest(json: &serde_json::Value, repo: &str) -> Option<MacRelease> {
    if !valid_repo(repo) {
        return None;
    }
    let version = json["version"].as_str()?.trim().trim_start_matches(['v', 'V']).to_string();
    let version_ok = !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit() || b == b'.');
    let build = json["build"].as_u64().filter(|b| *b > 0)?;
    let url = json["url"].as_str()?.trim();
    let sha256 = json["sha256"].as_str()?.trim().to_ascii_lowercase();
    if !version_ok
        || !url.starts_with(&download_prefix(repo))
        || url.contains("..")
        || sha256.len() != 64
        || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let release_page = format!("https://github.com/{}/releases", repo);
    let release_url = json["release_url"]
        .as_str()
        .map(str::trim)
        .filter(|u| u.starts_with(&release_page))
        .unwrap_or(&release_page)
        .to_string();
    let notes = json["notes"].as_str().map(str::trim).filter(|n| !n.is_empty());
    let mut info = UpdateInfo::new(
        format!("{}-mac.{}", version, build),
        notes.unwrap_or("No changelog available.").to_string(),
        json["published_at"].as_str().unwrap_or("").trim().to_string(),
        release_url,
    );
    info.appimage = Some(AppImageAsset {
        url: url.to_string(),
        sha256,
        size: json["size"].as_u64().filter(|s| *s > 0),
    });
    Some(MacRelease { version, build, info })
}

/// Whether `release` is newer than the running build: a higher upstream version, or the same
/// version with a higher build number.
pub fn is_newer_build(release: &MacRelease, current_version: &str, current_build: u64) -> bool {
    if is_newer_version(&release.version, current_version) {
        return true;
    }
    !is_newer_version(current_version, &release.version) && release.build > current_build
}

/// The newest macOS build, if it is newer than this one. `Ok(None)` also covers a fork that
/// has published no update document yet.
pub async fn check(client: &Client, repo: &str, current_version: &str, current_build: u64) -> Result<Option<UpdateInfo>, String> {
    let url = manifest_url(repo).ok_or("This build has no valid update address.")?;
    check_at(client, &url, repo, current_version, current_build).await
}

/// [`check`] against an explicit document address (tests point it at a loopback server).
pub async fn check_at(
    client: &Client,
    url: &str,
    repo: &str,
    current_version: &str,
    current_build: u64,
) -> Result<Option<UpdateInfo>, String> {
    let mut res = client
        .get(url)
        .header("User-Agent", APP_USER_AGENT)
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("Could not reach the update server: {}", crate::error::AppError::from(e).reason()))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("The update server answered HTTP {}", res.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| format!("Could not read the update information: {}", crate::error::AppError::from(e).reason()))?
    {
        if body.len() + chunk.len() > MAX_MANIFEST_BYTES {
            return Err("Unexpected response from the update server: the document is too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    let json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Unexpected response from the update server: {}", e))?;
    let release = parse_manifest(&json, repo).ok_or("The published update information is not usable.")?;
    Ok(is_newer_build(&release, current_version, current_build).then_some(release.info))
}

fn writable(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_or(false, |c| unsafe { libc::access(c.as_ptr(), libc::W_OK) } == 0)
}

/// The `.app` bundle the running program `exe` is in (`<bundle>/Contents/MacOS/<exe>`), when the
/// updater may replace it: the bundle and its folder can be written and Anix is not running as
/// root (`euid`). Otherwise the reason, shown in the update dialog.
pub fn bundle_target(exe: &Path, euid: u32) -> Result<PathBuf, String> {
    if euid == 0 {
        return Err("Anix is running as root; updates are only installed for a normal user.".to_string());
    }
    let bundle = exe
        .parent()
        .filter(|p| p.ends_with("Contents/MacOS"))
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|b| b.extension().is_some_and(|e| e == "app") && b.is_absolute())
        .ok_or("Anix is not running from an app bundle.")?
        .to_path_buf();
    // macOS runs a downloaded app that was never moved from a random read-only copy.
    if bundle.components().any(|c| c.as_os_str() == "AppTranslocation") {
        return Err("Move Anix into your Applications folder, open it from there, and check again.".to_string());
    }
    let folder = bundle.parent().ok_or("The app's folder could not be found.")?;
    if !writable(&bundle) || !writable(folder) {
        return Err(format!("Anix cannot write to {}.", folder.display()));
    }
    Ok(bundle)
}

/// A hidden name beside `bundle`: `.<bundle name><suffix>`.
fn hidden_sibling(bundle: &Path, suffix: &str) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(bundle.file_name().unwrap_or_default());
    name.push(suffix);
    bundle.with_file_name(name)
}

/// Where the previous version is kept after an update.
pub fn previous_path(bundle: &Path) -> PathBuf {
    hidden_sibling(bundle, ".previous")
}

fn remove_any(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// The one `.app` folder in `dir`, the unpacked update.
fn unpacked_app(dir: &Path) -> Option<PathBuf> {
    let mut apps = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_dir() && p.extension().is_some_and(|e| e == "app"));
    let app = apps.next()?;
    (apps.next().is_none() && app.join("Contents/MacOS").is_dir()).then_some(app)
}

/// Puts the app in `archive` in place of `bundle` once the archive's SHA-256 is `sha256`. The
/// archive is unpacked beside `bundle` with the system `tar`; the old bundle is renamed to
/// [`previous_path`] and the new one renamed into place, and if that second rename fails the old
/// bundle is put back. On any failure `bundle` is left as it was. Returns the previous bundle.
pub fn install_bundle(archive: &Path, bundle: &Path, sha256: &str) -> Result<PathBuf, String> {
    let staging = hidden_sibling(bundle, ".update");
    let fail = |message: String| {
        let _ = std::fs::remove_file(archive);
        remove_any(&staging);
        Err(message)
    };
    match sha256_file(archive) {
        Ok(hash) if hash.eq_ignore_ascii_case(sha256.trim()) => {}
        Ok(_) => return fail("The downloaded update does not match its checksum, so it was not installed. Anix is unchanged.".to_string()),
        Err(e) => return fail(format!("The downloaded update could not be read: {}", e)),
    }

    remove_any(&staging);
    if let Err(e) = std::fs::create_dir_all(&staging) {
        return fail(format!("Could not make room for the update next to {}: {}", bundle.display(), e));
    }
    let unpacked = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&staging)
        .output();
    match unpacked {
        Ok(out) if out.status.success() => {}
        Ok(out) => return fail(format!("The update could not be unpacked: {}", String::from_utf8_lossy(&out.stderr).trim())),
        Err(e) => return fail(format!("The update could not be unpacked: {}", e)),
    }
    let Some(new_app) = unpacked_app(&staging) else {
        return fail("The update does not contain the Anix app.".to_string());
    };

    let previous = previous_path(bundle);
    remove_any(&previous);
    if let Err(e) = std::fs::rename(bundle, &previous) {
        return fail(format!("The current version could not be moved aside, so nothing was changed: {}", e));
    }
    if let Err(e) = std::fs::rename(&new_app, bundle) {
        let _ = std::fs::rename(&previous, bundle);
        return fail(format!("The update could not replace {}: {}", bundle.display(), e));
    }
    let _ = std::fs::remove_file(archive);
    remove_any(&staging);
    Ok(previous)
}

/// Downloads `asset` beside `bundle` and installs it with [`install_bundle`]. `progress` hears
/// the fraction done. Returns the previous bundle.
pub async fn download_and_install(
    client: &Client,
    asset: &AppImageAsset,
    bundle: &Path,
    mut progress: impl FnMut(f64),
) -> Result<PathBuf, String> {
    let archive = hidden_sibling(bundle, ".update.tar.gz");
    let result = async {
        let mut res = client
            .get(&asset.url)
            .header("User-Agent", APP_USER_AGENT)
            .send()
            .await
            .map_err(|e| format!("Could not download the update: {}", crate::error::AppError::from(e).reason()))?;
        if !res.status().is_success() {
            return Err(format!("The update download answered HTTP {}", res.status().as_u16()));
        }
        let total = asset.size.or(res.content_length()).filter(|t| *t > 0);
        let mut file = tokio::fs::File::create(&archive)
            .await
            .map_err(|e| format!("Could not save the update next to {}: {}", bundle.display(), e))?;
        let mut written: u64 = 0;
        loop {
            let chunk = match tokio::time::timeout(STALL_TIMEOUT, res.chunk()).await {
                Ok(Ok(Some(chunk))) => chunk,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => return Err(format!("The update download was interrupted: {}", e)),
                Err(_) => return Err("The update download stalled.".to_string()),
            };
            written += chunk.len() as u64;
            if written > MAX_ARCHIVE_BYTES {
                return Err("The update download is larger than the app should be.".to_string());
            }
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| format!("Could not save the update: {}", e))?;
            if let Some(total) = total {
                progress((written as f64 / total as f64).min(1.0));
            }
        }
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| format!("Could not save the update: {}", e))?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&archive).await;
        return Err(e);
    }
    let (archive, bundle, sha256) = (archive.clone(), bundle.to_path_buf(), asset.sha256.clone());
    tokio::task::spawn_blocking(move || install_bundle(&archive, &bundle, &sha256))
        .await
        .map_err(|e| e.to_string())?
}
