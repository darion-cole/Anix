//! Anime4K upscaling for an external mpv on macOS. The embedded mpv player that applies the
//! Anime4K shaders on Linux does not exist on macOS, so there the upscaling chosen in the
//! player's settings (Upscaling › A/B/C, Fast/HQ) is given to mpv when Anix opens a stream, a
//! local file or a playlist in it, as a `--glsl-shaders` option naming the shaders bundled with
//! the app. The page tells the backend the current choice with `set_external_upscaling`
//! whenever it changes; `commands::player::sanitized_command` adds the option to every mpv it
//! starts.
//!
//! The pure parts ([`mpv_shader_arg`], [`bundled_shader_dir`]) are compiled and tested on every
//! system; the option is only added on macOS.

use crate::embedded_mpv::policy::{shader_list, Anime4k};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The upscaling the page last reported; `None` until it does, which means off.
static PRESET: Mutex<Option<Anime4k>> = Mutex::new(None);

/// Reads a preset as the page writes it (`off`, `a_fast` … `c_hq`). Anything else is `None`.
pub fn parse_preset(preset: &str) -> Option<Anime4k> {
    serde_json::from_value(serde_json::Value::String(preset.trim().to_string())).ok()
}

/// The page's current upscaling choice for external mpv. An unknown preset turns it off.
#[tauri::command]
pub fn set_external_upscaling(preset: String) -> Result<(), String> {
    let parsed = parse_preset(&preset);
    if parsed.is_none() {
        tracing::warn!("Unknown upscaling preset for external mpv: {:?}; upscaling is off", preset);
    }
    *PRESET.lock().map_err(|e| e.to_string())? = parsed;
    Ok(())
}

/// The `--glsl-shaders=…` option for `preset` with the shaders in `dir`, or `None` when the
/// preset is off or a shader file is missing (mpv would refuse to play with a missing shader).
pub fn mpv_shader_arg(dir: &Path, preset: Anime4k) -> Option<String> {
    if preset == Anime4k::Off {
        return None;
    }
    let list = shader_list(dir, preset).ok()?;
    if !list.split(':').all(|file| Path::new(file).is_file()) {
        tracing::warn!("Some Anime4K shaders are missing from {}; mpv starts without upscaling", dir.display());
        return None;
    }
    Some(format!("--glsl-shaders={}", list))
}

/// Where a macOS app bundle keeps the shaders, from the running program `exe`
/// (`<bundle>/Contents/MacOS/anix` → `<bundle>/Contents/Resources/resources/anime4k`).
pub fn bundled_shader_dir(exe: &Path) -> Option<PathBuf> {
    let contents = exe.parent().filter(|p| p.ends_with("Contents/MacOS"))?.parent()?;
    Some(contents.join("Resources/resources/anime4k"))
}

/// The option for the current choice, when this is macOS and upscaling is on.
pub fn current_mpv_shader_arg() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let preset = (*PRESET.lock().ok()?)?;
    let exe = std::env::current_exe().ok()?;
    mpv_shader_arg(&bundled_shader_dir(&exe)?, preset)
}
