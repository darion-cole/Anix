# Anix on macOS (unofficial port)

This fork builds Anix for macOS. The upstream project is Linux-only; the changes needed for
macOS live on the `macos` branch, and `main` stays an unchanged copy of upstream.

## Installing

1. Open the **Releases** page of this fork and download the `.dmg` from the newest
   "for macOS" release. It runs on both Apple Silicon and Intel Macs
   (macOS 11 or later).
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
| Video player | The built-in web player only. The embedded mpv player is Linux-only (it draws through GTK). |
| Local `.mkv` files | macOS's web view cannot play MKV. Use **Open in mpv/VLC** for those. MP4 and streams play in the app. |
| External mpv / VLC | Works. Anix finds Homebrew programs and `VLC.app` even when started from the Dock. |
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
   an update even when the Anix version stays the same (shown as `1.3.0-mac.<build>`).
3. **In the app**, Settings › Check for updates (and the automatic check) finds the new build.
   **Install** downloads it, checks its SHA-256, replaces Anix in its folder and keeps the
   previous version as a hidden `.Anix.app.previous` next to it. **Restart now** opens the
   new version. Your library and settings stay.

The app must be in a folder it can write to (Applications is fine for an admin account) and
must have been opened from there, not straight from the .dmg or Downloads.

**When a merge needs a person.** If upstream changed code that the macOS changes also touch,
or changed one of its own workflow files, the daily job cannot finish the merge. Its run fails
(GitHub emails you) and, if Issues are turned on for this fork, it opens an issue. Then ask
Claude to "merge upstream Anix <version> into the macos branch of darion-cole/Anix and fix the
conflicts". You can also start the job by hand from the Actions tab (**Sync upstream release ›
Run workflow**).

To roll back an update, quit Anix, delete `Anix.app` from Applications and rename the hidden
`.Anix.app.previous` back to `Anix.app` (in Finder, press ⌘⇧. to show hidden files).

## Where the macOS changes live

The changes are small and kept in a few places, so most upstream releases merge without conflicts:

- `src-tauri/tauri.macos.conf.json`, `src-tauri/Info.plist`: bundle settings.
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
- `src-tauri/src/embedded_mpv/mod.rs`: always the web player on macOS.
- `src-tauri/src/commands/player.rs`: `host_program`, finding mpv/VLC/ffmpeg on macOS.
- `src-tauri/src/commands/system.rs`: `open` instead of `xdg-open`.
- `src-tauri/src/commands/desktop.rs`: window buttons on the left.
- `src-tauri/src/system/hwaccel.rs`: a macOS note in the hardware report.
