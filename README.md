# yt-dlp Right Click

<p align="center">
  <img src="docs/assets/github-hero.png" alt="yt-dlp Right Click banner showing the extension popup and right-click presets">
</p>

<p align="center">
  <a href="https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/tag/v1.0.0"><img alt="Release" src="https://img.shields.io/badge/release-v1.0.0-14b8a6?style=for-the-badge"></a>
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows-2563eb?style=for-the-badge">
  <img alt="Chrome MV3" src="https://img.shields.io/badge/Chrome-MV3-f97316?style=for-the-badge">
  <img alt="Native host" src="https://img.shields.io/badge/native%20host-Rust-111827?style=for-the-badge">
</p>

## About

`yt-dlp Right Click` adds local `yt-dlp` download presets directly to Chrome's right-click menu and extension popup.

It is built for people who already use `yt-dlp` on Windows and want a quick browser-side launcher without copying URLs into a terminal. The extension captures the current page, link, selected URL, image, video, or audio URL, then sends it to a local Rust native messaging host.

Chrome extensions cannot launch local programs by themselves. This project uses Chrome's native messaging system: the browser extension handles the UI, and the native host handles local execution. The host validates every request, maps preset IDs to hardcoded `yt-dlp` arguments, and launches `yt-dlp.exe` directly without a shell.

## Highlights

- Right-click any page, link, selected URL, image, video, or audio element.
- Download from a compact popup when right-click capture is awkward.
- Use preset-only commands for MP4 video, MP3/M4A audio, best file, and explicit playlists.
- Track active jobs with the extension badge and recent jobs in the popup.
- Open completed download folders and retry failed jobs.
- Keep filenames clean with `Title.ext` output instead of noisy video IDs.
- Retry with Chrome cookies only when `yt-dlp` reports that login or cookies are required.
- Detect local JavaScript runtimes for modern YouTube extraction warnings.
- Run `yt-dlp -U` manually or once per day when enabled.

## Download

Grab the latest GitHub release:

[Download v1.0.0](https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/tag/v1.0.0)

Release assets:

- `yt-dlp-right-click-extension-v1.0.0.zip`
- `yt-dlp-right-click-native-host-windows-v1.0.0.zip`

The native host ZIP includes a prebuilt `ytdlp_native_host.exe`. Release users do not need Rust, Cargo, or a local build step.

## Prerequisites

| Requirement | Why it is needed | Notes |
| --- | --- | --- |
| Windows | The native host and installer target Windows. | User-level install; no administrator rights required. |
| Google Chrome | The extension is a Chrome MV3 extension. | Chromium-based browsers may work if they support Chrome native messaging, but Chrome is the supported target. |
| `yt-dlp.exe` | Performs the actual download. | Set either the full `yt-dlp.exe` path or its containing folder in extension settings. |
| `ffmpeg.exe` | Required for MP4 merging and audio extraction presets. | Set either the full `ffmpeg.exe` path or the folder containing it. |
| Download folder | Where output files, logs, and subfolders are written. | The host creates `Video`, `Audio`, `Files`, and `_yt-dlp-right-click-logs` under this root. |
| Optional JavaScript runtime | Helps with some modern YouTube extraction warnings. | Leave on `Auto`; Node.js, Deno, QuickJS, or Bun can be detected. |

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
| Audio | MP3, M4A |
| File | Best available |
| Playlist | Best MP4, MP3 audio |

Playlists are only enabled from explicit playlist menu items. Normal video/audio/file presets use `--no-playlist`.

## Quick Install

These steps target Google Chrome on Windows.

1. Install or unpack the Chrome extension.
2. Download and unzip `yt-dlp-right-click-native-host-windows-v1.0.0.zip`.
3. Double-click `install-native-host.cmd`.
4. Paste the extension ID if prompted.
5. Open the extension settings page.
6. Set paths for `yt-dlp.exe`, `ffmpeg.exe`, and your download folder.
7. Click `Test native host and settings`.

The native host must be installed once so Chrome knows which local executable is allowed to receive messages from this extension. It does not have to be built by the user.

For development from source:

```powershell
cd native-host
cargo build --release
cd ..
.\scripts\install-native-host.ps1 -ExtensionId "<your-extension-id>"
```

See [docs/INSTALL.md](docs/INSTALL.md) for full setup and troubleshooting.

## Security Model

The extension can collect a URL and preset ID. It cannot choose arbitrary command-line arguments.

The native host:

- accepts only `http://` and `https://` URLs
- rejects `file:`, `javascript:`, `data:`, `chrome:`, and `blob:` URLs
- validates paths as absolute local paths
- launches `yt-dlp.exe` directly with an argument array
- restricts final output and `Open folder` actions to the configured download root
- excludes generated native host manifests from release artifacts

See [SECURITY.md](SECURITY.md) for the trust boundary and expected limits.

## Project Layout

```text
extension/       Chrome MV3 extension
native-host/     Rust native messaging host
scripts/         Windows install, uninstall, and release packaging scripts
docs/            Install and release documentation
```

## Build Release Artifacts

```powershell
.\scripts\package-release.ps1 -Version 1.0.0
```

The script creates extension and native-host ZIP files under `dist\1.0.0`. The native host ZIP is assembled from a whitelist, so local generated manifests are not included.

## Public Build Note

Do not publish `native-host/com.ytdlp_right_click.native_host.json` from a local machine. It is generated by the install script and contains an absolute user-specific executable path plus the installed Chrome extension ID.

Use `native-host/com.ytdlp_right_click.native_host.template.json` as documentation/template only.
