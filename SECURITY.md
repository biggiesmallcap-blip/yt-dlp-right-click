# Security

This project intentionally separates browser input from local command execution.

## Trust Boundary

The Chrome extension can collect a URL and preset ID. It cannot decide the executable command line. The native host validates the request and builds the final `yt-dlp` argument list from hardcoded presets.

## Local Execution Rules

- The native host never runs through `cmd.exe`, PowerShell, or a shell string.
- It launches `yt-dlp.exe` directly with an argument array.
- Preset IDs are allowlisted.
- Arbitrary custom `yt-dlp` arguments are not supported in v1.
- Only `http://` and `https://` URLs are accepted.
- Direct `file:`, `javascript:`, `data:`, `chrome:`, and `blob:` URLs are rejected.
- `yt-dlp.exe` or its containing folder, ffmpeg, and the download folder must be absolute local paths.
- JavaScript runtime auto-detection checks PATH and common Windows install folders. Manual runtime paths for `--js-runtimes` must be absolute local paths and must already exist.
- Download output goes only under the configured download root.
- The update action can only run the configured `yt-dlp.exe` with exactly `-U`.
- Automatic updates are not started while downloads are active.
- Extension pages use a restrictive MV3 content security policy: local scripts only, no plugins.
- The `activeTab` permission is used only by the popup fallback button to read the current tab URL after the user clicks the extension button.
- `Open folder` only opens files or folders under the configured download folder.
- Post-download cleanup only removes matching yt-dlp temp artifacts from the job's output folder. It does not clear browser caches, global yt-dlp caches, or arbitrary files under the download root.

## Native Messaging Host

Chrome will only start the native host for extension IDs listed in the host manifest `allowed_origins`. The checked-in template is `native-host/com.ytdlp_right_click.native_host.template.json`; the real `native-host/com.ytdlp_right_click.native_host.json` is generated during install because it must contain the user's absolute executable path and the actual extension ID.

The install script writes a user-level registry entry under:

```text
HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ytdlp_right_click.native_host
```

Do not add wildcard origins or unrelated extension IDs to the host manifest. Do not publish a generated manifest from a developer machine.

For public releases, embed the Chrome Web Store extension ID into `scripts/install-native-host.ps1` before packaging the native host installer. Source builds can still prompt for the unpacked extension ID.

## Cookies

Cookie mode defaults to `Auto`. In auto mode, the host first runs `yt-dlp` without Chrome cookies. If yt-dlp reports that login/cookies are required, the host retries once with:

```text
--cookies-from-browser chrome
```

This may allow `yt-dlp` to access logged-in browser sessions only on the retry path. `Never` and `Always` modes are available in settings for troubleshooting.

## Logs

The host writes per-job logs under the configured download folder in `_yt-dlp-right-click-logs`. Logs contain process output but not a reconstructed command line. Cookie mode is recorded only as enabled/disabled.

## Expected Limits

This project does not bypass DRM, paywalls, or site restrictions. Final site support depends on `yt-dlp`.

## Updates

Manual and optional daily updates run:

```text
yt-dlp.exe -U
```

This is intentionally a fixed native-host action, not a custom argument field. If `yt-dlp` is installed through pip, winget, scoop, Chocolatey, or another package manager, that manager may reject or override `yt-dlp -U`; leave automatic updates off in that setup.
