# yt-dlp Right Click

<p align="center">
  <img src="docs/assets/github-hero.png" alt="yt-dlp Right Click banner">
</p>

<p align="center">
  <a href="https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/biggiesmallcap-blip/yt-dlp-right-click?style=for-the-badge&color=14b8a6"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-2563eb?style=for-the-badge">
  <img alt="Chrome MV3" src="https://img.shields.io/badge/Chrome-MV3-f97316?style=for-the-badge">
  <img alt="Native host" src="https://img.shields.io/badge/native%20host-Rust-111827?style=for-the-badge">
</p>

## About

`yt-dlp Right Click` adds local `yt-dlp` download presets directly to Chrome's right-click menu and extension popup.

It is built for people who already use `yt-dlp` on Windows and want a quick browser-side launcher without copying URLs into a terminal. The extension captures the current page, link, selected URL, image, video, or audio URL, then sends it to a local Rust native messaging host.

Chrome extensions cannot launch local programs by themselves. This project uses Chrome's native messaging system: the browser extension handles the UI, and the native host handles local execution. The host validates every request, maps preset IDs to hardcoded `yt-dlp` arguments, and launches `yt-dlp.exe` directly without a shell.

## What's New in 1.1

Live progress with Cancel and Resume, a redesigned popup and settings page, automatic yt-dlp updates, Instagram backup presets, MP3 320 kbps, tagged audio with cover art, notifications, and a security fix: yt-dlp now ignores config files, so a downloaded file can no longer inject yt-dlp options. Full list in [CHANGELOG.md](CHANGELOG.md).

Upgrading from 1.0? Install the new native host too (see [Upgrading](#upgrading-from-100)); the 1.0 host does not know the new presets.

## Highlights

<p align="center">
  <img src="docs/assets/popup-v1.1.jpg" width="380" alt="The popup in dark mode: a preset picker with a Download button, two downloads in progress with segmented progress meters, speed and time left, finished downloads with Show in folder, and a failed download explaining the cookie fix">
</p>

- Right-click any page, link, selected URL, image, video, or audio element, or use "Download again" to repeat your last preset.
- Live progress, speed, and time left in the popup, with Cancel and Resume.
- Preset-only commands for MP4 video, MP3 (VBR or 320 kbps), M4A, original audio, best file, playlists, and Instagram.
- Audio files tagged with title, artist, and cover art.
- Desktop notifications when downloads finish; click one to open the folder.
- "Show in folder" selects the saved file in Explorer.
- Clean `Title.ext` filenames instead of noisy video IDs.
- Cookies only when a site asks for login, from your choice of browser or a cookies.txt file.
- yt-dlp kept up to date automatically (stable or nightly), plus a notice when a new extension release is out.
- "Check setup" reports native host, yt-dlp, and ffmpeg versions and warns when something is stale or misconfigured.

## Download

Grab the latest GitHub release:

[Download the latest release](https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/latest)

Release assets:

- `yt-dlp-right-click-extension-v<version>.zip`
- `yt-dlp-right-click-native-host-windows-v<version>.zip`

The native host ZIP includes a prebuilt `ytdlp_native_host.exe`. Release users do not need Rust, Cargo, or a local build step.

## Prerequisites

| Requirement | Why it is needed | Notes |
| --- | --- | --- |
| Windows | The native host and installer target Windows. | User-level install; no administrator rights required. |
| Google Chrome | The extension is a Chrome MV3 extension. | Chromium-based browsers may work if they support Chrome native messaging, but Chrome is the supported target. |
| `yt-dlp.exe` | Performs the actual download. | Set either the full `yt-dlp.exe` path or its containing folder in extension settings. |
| `ffmpeg.exe` | Required for MP4 merging and audio extraction presets. | Set either the full `ffmpeg.exe` path or the folder containing it. |
| Download folder | Where output files, logs, and subfolders are written. | The host creates `Video`, `Audio`, `Files`, `Instagram`, and `_yt-dlp-right-click-logs` under this root. Logs older than 30 days are pruned. |
| JavaScript runtime | Modern YouTube extraction needs one. | Leave on `Automatic`; Node.js, Deno, QuickJS, or Bun are detected. |

What is bundled:

- Chrome extension files in the extension ZIP.
- A prebuilt `ytdlp_native_host.exe` in the Windows native host ZIP.
- Installer scripts that register the native host for the current Windows user.

What is not bundled:

- `yt-dlp.exe`
- `ffmpeg.exe`
- browser cookies or account credentials
- any DRM, paywall, or site-restriction bypass

## Presets

| Group | Presets |
| --- | --- |
| Video | Best MP4, MP4 up to 1080p, Small MP4 up to 720p |
| Audio | MP3 (VBR best), MP3 320 kbps, M4A, Original audio (no re-encode) |
| File | Best single file |
| Playlist | Whole playlist as MP4, Whole playlist as MP3 |
| Instagram | Post, reel, or carousel; Stories and highlights backup |

Playlists are only enabled from explicit playlist menu items. Normal video/audio/file presets use `--no-playlist`.

The Instagram group only appears on instagram.com and is meant for backing up your own account. It uses your logged-in cookies and saves to `Instagram\<account>\`. The stories backup keeps an archive so repeat runs only fetch new items. Right-click a profile page to back up that profile's current stories, or a highlight to save the highlight.

## Install

Windows and Google Chrome, about 5 minutes. You set up three things: yt-dlp itself, the Chrome extension, and a small helper program that lets the extension start yt-dlp.

### Before you start: get yt-dlp and ffmpeg

- **yt-dlp:** download `yt-dlp.exe` from the [yt-dlp releases page](https://github.com/yt-dlp/yt-dlp/releases/latest) and put it in a folder, for example `C:\Tools\yt-dlp\`.
- **ffmpeg:** download a Windows build from [yt-dlp's FFmpeg builds](https://github.com/yt-dlp/FFmpeg-Builds/releases/latest) (the `win64-gpl` zip), unzip it, and note the `bin` folder that contains `ffmpeg.exe`.
- **For YouTube:** also install [Deno](https://deno.com/) or [Node.js](https://nodejs.org/). The extension finds them automatically.

### Step 1: Add the extension to Chrome

1. From the [latest release](https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/latest), download `yt-dlp-right-click-extension-v<version>.zip`.
2. Unzip it into a folder you will keep, for example `C:\Tools\yt-dlp-right-click\extension\`. Chrome runs the extension from this folder, so don't delete or move it later.
3. In Chrome, go to `chrome://extensions` and turn on **Developer mode** (top right).
4. Click **Load unpacked** and select the folder from step 2.
5. Copy the extension's **ID**: the 32-letter code shown on its card, like `nbimhdbidaggcplfdalkjelidpomghlf`. You need it in Step 2.

### Step 2: Install the helper program

Chrome extensions can't run programs on your PC by themselves. This helper (the "native host") lets this one extension, and nothing else, start yt-dlp. No admin rights needed.

1. From the same release, download `yt-dlp-right-click-native-host-windows-v<version>.zip`.
2. Unzip it into a folder you will keep, for example `C:\Tools\yt-dlp-right-click\native-host\`.
3. Double-click `install-native-host.cmd`. If Windows shows "Windows protected your PC", click **More info**, then **Run anyway**.
4. When it says `Paste extension ID`, paste the ID from Step 1 and press Enter.
5. You should see `Registered native host`. Press any key to close the window.

If you ever move this folder, run `install-native-host.cmd` again.

### Step 3: Tell it where yt-dlp and ffmpeg are

1. Click the extension's icon in the Chrome toolbar, then the gear icon. (Pin it first from the puzzle-piece menu if you can't see it.)
2. Fill in:
   - **yt-dlp:** the folder containing `yt-dlp.exe`
   - **ffmpeg:** the folder containing `ffmpeg.exe`
   - **Download folder:** where downloads should go
3. Click **Check setup**. It lists the versions it found, or tells you exactly what's wrong.

### Step 4: Download something

Right-click a video, a link, or the page itself, choose **yt-dlp**, and pick a format. Or click the extension icon and press **Download** to grab the current tab. Progress shows in the popup.

### Building from source instead

For development, build the helper yourself and register it with your extension ID:

```powershell
cd native-host
cargo build --release
cd ..
.\scripts\install-native-host.ps1 -ExtensionId "<your-extension-id>"
```

See [docs/INSTALL.md](docs/INSTALL.md) for full setup and troubleshooting.

## Upgrading from 1.0.0

1. Download both 1.1.0 release ZIPs.
2. Replace your unpacked extension folder with the new extension ZIP contents, then click reload on `chrome://extensions`. The extension ID stays the same as long as you load it from the same folder.
3. Unzip the new native host package and run `install-native-host.cmd` again.
4. Open Settings and click `Check setup`. It should report native host v1.1.0.

From source: pull, then `cargo build --release` in `native-host` and reload the extension.

Settings carry over. Automatic yt-dlp updates are switched on once during the upgrade; turn them off in Settings if a package manager owns yt-dlp.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| Popup says the native host is older than the extension | Install the latest native host package, or rebuild from source. |
| "Could not read chrome cookies" or "Failed to decrypt with DPAPI" | Chrome, Edge, and Brave on Windows encrypt cookies in a way yt-dlp often cannot read. Export a cookies.txt file and set it under Settings > Cookies, or choose Firefox as the cookie browser. |
| "Sign in to confirm you're not a bot" | Usually fixed by updating yt-dlp (Settings > Update yt-dlp now), then by cookies. |
| Audio conversion or merging fails | Update ffmpeg; very old builds (2020 and earlier) fail on some formats and on long paths. |
| A download stopped when the browser closed | Expected: closing Chrome or reloading the extension stops running downloads. Retry resumes from the partial file. |

## Security Model

The extension can collect a URL and preset ID. It cannot choose arbitrary command-line arguments.

The native host:

- accepts only `http://` and `https://` URLs
- rejects `file:`, `javascript:`, `data:`, `chrome:`, and `blob:` URLs
- validates paths as absolute local paths
- launches `yt-dlp.exe` directly with an argument array
- passes the URL after `--` so it can never be read as an option
- runs yt-dlp with `--ignore-config`, so no config file (including one a download might drop into the output folder) can add options such as `--exec`
- allowlists every setting that reaches the command line (cookie browser, update channel, JS runtime)
- reads titles and file paths back as JSON, so page-controlled titles cannot spoof progress or results
- stops yt-dlp when the browser disconnects instead of leaving it running unattended
- restricts final output, `Show in folder`, and `Show log file` to the configured download root
- excludes generated native host manifests from release artifacts

See [SECURITY.md](SECURITY.md) for the trust boundary and expected limits.

## Project Layout

```text
extension/       Chrome MV3 extension (shared.js holds presets and pure helpers)
native-host/     Rust native messaging host
scripts/         Windows install, uninstall, and release packaging scripts
docs/            Install and release documentation
tests/           Node unit tests for the extension helpers
```

## Build Release Artifacts

```powershell
.\scripts\package-release.ps1 -Version 1.1.0
```

The script creates extension and native-host ZIP files under `dist\1.1.0`. The native host ZIP is assembled from a whitelist, so local generated manifests are not included.

## Public Build Note

Do not publish `native-host/com.ytdlp_right_click.native_host.json` from a local machine. It is generated by the install script and contains an absolute user-specific executable path plus the installed Chrome extension ID.

Use `native-host/com.ytdlp_right_click.native_host.template.json` as documentation/template only.
