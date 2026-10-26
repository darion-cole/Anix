# Anix on macOS (unofficial port)

This fork builds Anix for macOS. The upstream project is Linux-only; the changes needed for
macOS live on the `macos` branch, and `main` stays an unchanged copy of upstream.

## Installing

1. Open the **Releases** page of this fork and download `Anix_<version>_macOS_universal.dmg`
   from the newest "for macOS" release. It runs on both Apple Silicon and Intel Macs
   (macOS 11 or later).
2. Open the .dmg and drag **Anix** into **Applications**.
3. The app is not notarized by Apple, so the first launch is blocked. Either:
   - right-click Anix in Applications, choose **Open**, then **Open** again; or
   - try to open it once, then go to **System Settings › Privacy & Security** and click
     **Open Anyway**.

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
| In-app updater | Not available. Updates come from this fork's releases (below). |
| App data | `~/Library/Application Support/anix` |

## Getting upstream updates

1. On GitHub, open this fork and click **Sync fork** on `main` to pull the latest upstream code.
2. Merge `main` into `macos` (or ask Claude to "merge the latest upstream Anix into the
   macos branch of darion-cole/Anix and fix any conflicts").
3. Pushing to `macos` runs the **macOS build** workflow, which publishes a new release with
   the .dmg. Download it and replace the app in Applications. Your library and settings stay.

The macOS changes are small and kept in a few places, so most updates merge without conflicts:

- `src-tauri/tauri.macos.conf.json`, `src-tauri/Info.plist`: bundle settings.
- `src-tauri/icons/macos/icon.icns`: a real macOS icon (the shared `icons/icon.icns` is a PNG
  with the wrong extension, which macOS shows as a blank grey icon).
- `.github/workflows/macos.yml`: the build.
- `src-tauri/src/notifier.rs`: notifications without the Linux click action.
- `src-tauri/src/mpris.rs`: MPRIS commands do nothing on macOS.
- `src-tauri/src/embedded_mpv/mod.rs`: always the web player on macOS.
- `src-tauri/src/commands/player.rs`: `host_program`, finding mpv/VLC/ffmpeg on macOS.
- `src-tauri/src/commands/system.rs`: `open` instead of `xdg-open`.
- `src-tauri/src/commands/desktop.rs`: window buttons on the left.
- `src-tauri/src/system/hwaccel.rs`: a macOS note in the hardware report.
