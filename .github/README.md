<div align="center">

<img src="../public/logo.png" alt="Anix logo" width="120" />

# Anix for macOS

### An unofficial macOS port of Anix, the anime catalogue, tracker and local library player

[![Platform](https://img.shields.io/badge/Platform-macOS%2011%2B%20(Apple%20Silicon%20%7C%20Intel)-000000?logo=apple&logoColor=white)](https://github.com/darion-cole/Anix/releases/latest)
[![Upstream](https://img.shields.io/badge/Upstream-MaidLover69%2FAnix-7c3aed?logo=github)](https://github.com/MaidLover69/Anix)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](../LICENSE)

</div>

---

## Credit

**Anix is made by [MaidLover69](https://github.com/MaidLover69).** All of the app's features,
design and code come from the original project:

- Original repository: <https://github.com/MaidLover69/Anix> (also on [Codeberg](https://codeberg.org/MaidLover69/Anix))
- Website: <https://anix-linux.vercel.app>

The original Anix is a Linux app. This fork only adds what is needed to build and run it on
macOS, and builds it automatically. It is not affiliated with or supported by the original
author, so please report macOS-specific problems here rather than upstream. If you use Linux,
get Anix from the original project.

Anix is MIT-licensed, © 2026 MaidLover69. The licence and the third-party notices are kept
unchanged in [`LICENSE`](../LICENSE) and [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md).

---

## What Anix does

- **Catalogue:** browse and search AniList, with details, episodes, characters and recommendations.
- **Tracking:** AniList, MyAnimeList and Comick sync, watch history with resume, and stats.
- **Schedule:** weekly airing calendar with release notifications.
- **Local library:** scans your anime folders, matches files to titles, playlists and subtitles.
- **Player:** subtitles (ASS, SRT, WebVTT), AniSkip, dialogue boost, mini-player, or open in mpv / VLC.
- **Themes and languages:** 9 themes and 11 interface languages. No telemetry.

| Home | Anime page |
| :---: | :---: |
| ![Home](../screenshots/home.png) | ![Anime page](../screenshots/detail.png) |
| **Player** | **Schedule** |
| ![Player](../screenshots/player.png) | ![Schedule](../screenshots/schedule.png) |

Screenshots are from the original project.

---

## Install on macOS

Works on macOS 11 (Big Sur) or later, on both Apple Silicon and Intel Macs.

1. Download the `.dmg` from the [latest release](https://github.com/darion-cole/Anix/releases/latest).
2. Open it and drag **Anix** into **Applications**.
3. Open Anix from Applications. Because the app is not notarized by Apple, macOS shows
   "Apple could not verify Anix…". Click **Done**, open **System Settings › Privacy &
   Security**, scroll to **Security** and click **Open Anyway**, then confirm.

   Or run this once in Terminal, then open Anix normally:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Anix.app
   ```

Optional, for external players and downloads (with [Homebrew](https://brew.sh)):

```sh
brew install mpv ffmpeg
brew install --cask vlc
```

### Updates

You install the `.dmg` once. After that, Anix checks for a new build each time it starts and
offers it in a dialog (you can also check from **Settings › Check for updates**):
**Install** downloads the new build, checks it against its published checksum, replaces Anix
in place and keeps the previous version as a hidden backup. **Restart now** opens the new
version. Your library and settings are kept.

New releases of the original Anix are merged into this fork automatically every day, and each
build runs the test suite on a Mac before it is published.

---

## Differences from the Linux version

| Feature | On macOS |
|---|---|
| Video player | The built-in web player. The embedded mpv player is Linux-only. |
| Local `.mkv` files | Open them in mpv or VLC; macOS's web view cannot play MKV. MP4 and streams play in the app. |
| Media keys / lock-screen controls | Not available yet. |
| Clicking a release notification | Shows the notification, but does not open the anime. |
| "Alerts while Anix is closed" | Not available yet. |
| App data | `~/Library/Application Support/anix` |

For how the port works, how updates are built and how to roll back, see [MACOS.md](../MACOS.md).
The original project's README is [here](../README.md).
