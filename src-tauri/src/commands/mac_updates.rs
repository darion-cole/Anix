//! The update commands for the macOS build. `commands/updates.rs` hands each call over to the
//! function of the same name here on macOS, so the update dialog works unchanged: the check
//! reads the fork's `mac-update.json`, Install replaces the `.app` bundle, and Restart opens the
//! new bundle (see `clients/mac_update.rs`).

use crate::clients::mac_update;
use crate::commands::updates::{UpdateInstallSupport, UPDATE_PROGRESS_EVENT};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, State};

fn running_bundle() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find the running app: {}", e))?;
    mac_update::bundle_target(&exe, unsafe { libc::geteuid() })
}

fn update_repo() -> Result<&'static str, String> {
    mac_update::UPDATE_REPO
        .filter(|r| !r.trim().is_empty())
        .ok_or_else(|| "This build of Anix was not made by the macOS workflow, so it has no in-app updates.".to_string())
}

pub async fn check_for_update(state: State<'_, AppState>) -> Result<Option<crate::clients::update::UpdateInfo>, String> {
    let Ok(repo) = update_repo() else { return Ok(None) };
    mac_update::check(&state.client, repo, env!("CARGO_PKG_VERSION"), mac_update::current_build()).await
}

pub fn get_update_install_support() -> UpdateInstallSupport {
    // `appimage` is the dialog's "Install is possible" flag; on macOS it means the app bundle.
    match update_repo().and_then(|_| running_bundle()) {
        Ok(_) => UpdateInstallSupport { appimage: true, reason: None },
        Err(reason) => UpdateInstallSupport { appimage: false, reason: Some(reason) },
    }
}

pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let repo = update_repo()?;
    let bundle = running_bundle()?;
    // The address and checksum are read again here; the page cannot supply them.
    let info = mac_update::check(&state.client, repo, env!("CARGO_PKG_VERSION"), mac_update::current_build())
        .await?
        .ok_or("This is already the newest macOS build.")?;
    let asset = info.appimage.ok_or("The newest macOS build has no app archive.")?;
    let mut last = 0.0;
    // The archive is tens of megabytes: the download client has no total-time limit.
    let client = crate::clients::http::download_client();
    mac_update::download_and_install(&client, &asset, &bundle, |fraction| {
        if fraction - last >= 0.01 || fraction >= 1.0 {
            last = fraction;
            let _ = app.emit(UPDATE_PROGRESS_EVENT, fraction);
        }
    })
    .await?;
    tracing::info!("Installed macOS build {} into {}", info.version, bundle.display());
    Ok(bundle.to_string_lossy().into_owned())
}

/// Quits and opens the updated bundle. The new copy starts after this one has gone, so the
/// single-instance check does not hand it back to the old window.
pub fn restart_after_update(app: AppHandle) -> Result<(), String> {
    let bundle = running_bundle()?;
    crate::commands::player::sanitized_command("sh")
        .args(["-c", "sleep 2; exec open -n \"$0\""])
        .arg(&bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start the new version: {}", e))?;
    app.exit(0);
    Ok(())
}
