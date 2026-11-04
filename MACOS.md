# Anix on macOS (unofficial port)

This fork builds Anix for macOS. The upstream project is Linux-only; the changes needed for
macOS live on the `macos` branch, and `main` stays an unchanged copy of upstream.

## Installing

1. Open the **Releases** page of this fork and download the `.dmg` from the newest
   "for macOS" release. It runs on Apple Silicon Macs (M1 or later,
   macOS 11 or later). Intel Macs are not supported.
2. Open the .dmg and drag **Anix** into **Applications**.
3. The app is not notarized by Apple, so the first launch is blocked ("Apple could not
   verify Anix…"). Click **Done**, then go to **System Settings › Privacy & Security**, scroll
   to Security and click **Open Anyway**. (On macOS 14 and earlier, right-click the app and
   choose **Open** also works.)

   If macOS says the app "is damaged", run this once in Terminal, then open it normally:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Anix.app
   ```

Optional, for external players and downloads (install with [Homebrew](https://brew.sh)):

```sh
brew install mpv ffmpeg
brew install --cask vlc
```

## What differs from Linux

| Feature | macOS |
|---|---|
| Video player | Embedded mpv, as on Linux: the app bundles an LGPL libmpv and draws the video under the page (`embedded_mpv/layer_macos.rs`). Settings › Playback › Player engine can switch to the web player, and Anix falls back to it by itself when mpv cannot start. |
| Local `.mkv` files | Play in the embedded player. (The web player cannot play MKV.) |
| External mpv / VLC | Works. Anix finds Homebrew programs and `VLC.app` even when started from the Dock. |
| Anime4K upscaling | Works in the embedded player, and in external mpv. Fast, Balanced and HQ; Balanced (macOS fork addition) keeps HQ's first stage at about half its GPU load. |
| Media keys / lock screen controls | Not available (Linux MPRIS). |
| Release notifications | Shown, but clicking one does not open the anime. |
| "Alerts while Anix is closed" | Not functional on macOS (it uses Linux login services). |
| In-app updater | Settings › Check for updates installs new macOS builds in place (below). |
| App data | `~/Library/Application Support/anix` |

## Updates

You install the .dmg once. After that, updates arrive like this:

1. **Upstream releases are merged automatically.** The **Sync upstream release** workflow runs
   daily, merges the newest release of the original Anix into the `macos` branch and starts
   the macOS build. Only published releases are merged, not every upstream commit.
2. **Every build of `macos` is published as a release** with the .dmg, the app as
   `.app.tar.gz`, and `mac-update.json`. Builds are numbered, so a macOS-only fix is offered as
   an update even when the Anix version stays the same (shown as `<version>.<build>`, for example `1.4.0.26`).
3. **In the app**, the check at launch finds the new build and opens the update dialog by
   itself, once for each new build (it waits until you leave the player or close another dialog). You
   can also check any time from Settings › Check for updates.
   **Install** downloads it, checks its SHA-256, replaces Anix in its folder and keeps the
   previous version as a hidden `.Anix.app.previous` next to it. **Restart now** opens the
   new version. Your library and settings stay.

The app must be in a folder it can write to (Applications is fine for an admin account) and
must have been opened from there, not straight from the .dmg or Downloads.

**Releases that change workflow files.** GitHub never lets the job's own token push changes
to `.github/workflows`. With a repository secret named `SYNC_TOKEN` (a fine-grained token for
this repository only, with **Contents** and **Workflows** set to read and write), the daily job
pushes those merges too. Without it, such a release needs a manual merge. The token expires;
when it does, the run fails with a message saying so, and a new token fixes it.

**When a merge needs a person.** If upstream changed code that the macOS changes also touch,
or changed a workflow file with no working `SYNC_TOKEN`, the daily job cannot finish the merge. Its run fails
(GitHub emails you) and, if Issues are turned on for this fork, it opens an issue. Then ask
Claude to "merge upstream Anix <version> into the macos branch of darion-cole/Anix and fix the
conflicts". You can also start the job by hand from the Actions tab (**Sync upstream release ›
Run workflow**).

To roll back an update, quit Anix, delete `Anix.app` from Applications and rename the hidden
`.Anix.app.previous` back to `Anix.app` (in Finder, press ⌘⇧. to show hidden files).

## Where the macOS changes live

The changes are small and kept in a few places, so most upstream releases merge without conflicts:

- `src-tauri/tauri.macos.conf.json`, `src-tauri/Info.plist`: bundle settings.
  `app.macOSPrivateApi` turns on WebKit's element fullscreen (the player's fullscreen button);
  the Tauri CLI adds the matching `macos-private-api` feature to `Cargo.toml` while it builds.
- `src-tauri/icons/macos/icon.icns`: a real macOS icon (the shared `icons/icon.icns` is a PNG
  with the wrong extension, which macOS shows as a blank grey icon).
- `.github/workflows/macos.yml`: the build and release, with the update files.
- `.github/workflows/sync-upstream.yml`: the daily merge of upstream releases.
- `src-tauri/src/clients/mac_update.rs`, `src-tauri/src/commands/mac_updates.rs`: the in-app
  updater (tests in `src-tauri/tests/mac_update_tests.rs`). `commands/updates.rs` hands over to
  it on macOS, and `components/UpdateDialog.tsx` shows a macOS hint
  (`updateDialog.installHintMac` in every locale).
- `src-tauri/src/notifier.rs`: notifications without the Linux click action.
- `src-tauri/src/mpris.rs`: MPRIS commands do nothing on macOS.
- `src-tauri/src/embedded_mpv/mod.rs`: `layer_macos` on macOS, the bundled libmpv path
  (`macos_library_path`), and the engine default; `layer_macos.rs` (new) draws the video on a
  render thread under the webview.
- `scripts/build-libmpv-lgpl-macos.sh`, `.github/workflows/macos-libmpv.yml` (both new): the LGPL
  libmpv for Apple Silicon, cached in Actions and bundled in `Contents/Frameworks` by `macos.yml`.
- `src-tauri/Entitlements.plist`: `disable-library-validation`, so the ad-hoc signed app may load
  its bundled libmpv.
- `src-tauri/Cargo.toml`: `objc2`/`objc2-app-kit` for macOS only.
- `embedded_mpv/policy.rs`, `src/lib/anime4k.ts`, `PlayerSettingsPanel.tsx`: the Balanced quality
  (`playerSettingsPanel.balanced` in every locale); `policy.rs` also uses `demuxer-cache-dir`
  (mpv 0.41 dropped `cache-dir`, which broke the disk cache on every system).
- `src-tauri/src/commands/player.rs`: `host_program`, finding mpv/VLC/ffmpeg on macOS.
- `src-tauri/src/commands/player.rs`: `desktop_opener`, `open` instead of `xdg-open` for links,
  sign-in and folders (used by `open_in_browser` there and by `commands/system.rs`).
- `src-tauri/src/commands/desktop.rs`: window buttons on the left.
- `src-tauri/src/system/hwaccel.rs`: a macOS note in the hardware report.
- `src-tauri/src/commands/external_upscaling.rs` (new), one line in `sanitized_command`, one
  entry in `lib.rs`, `src/hooks/useExternalUpscaling.ts`, `src/lib/platform.ts` and the Upscaling
  section of `PlayerSettingsPanel.tsx` (`playerSettingsPanel.macMpvUpscaling` in every locale):
  Anime4K for external mpv.
- `src-tauri/src/system/mod.rs`: `on_ui_main_thread`, the macOS main-thread check used by `restart`
  (upstream asks GTK, which macOS does not have).

## Libraries bundled in the macOS app

The embedded player's libraries are built from source by `scripts/build-libmpv-lgpl-macos.sh`
and their licence texts ship in `Anix.app/Contents/Resources/libmpv-licenses`:

| Component | Licence |
|---|---|
| mpv (built with `-Dgpl=false`) | LGPL-2.1+ |
| FFmpeg (no GPL or non-free parts) | LGPL-2.1+ |
| libplacebo | LGPL-2.1+ |
| FriBidi | LGPL-2.1+ |
| libass | ISC |
| HarfBuzz | MIT |
| FreeType | FreeType License (FTL) |
| dav1d | BSD-2-Clause |

They are dynamically linked and replaceable (in `Contents/Frameworks`), as the LGPL asks.
Portions of this software are copyright © The FreeType Project (https://freetype.org). All rights reserved.
