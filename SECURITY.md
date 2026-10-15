# Security

This project intentionally separates browser input from local command execution.

## Trust Boundary

The Chrome extension can collect a URL and preset ID. It cannot decide the executable command line. The native host validates the request and builds the final `yt-dlp` argument list from hardcoded presets.

## Local Execution Rules

- The native host never runs through `cmd.exe`, PowerShell, or a shell string.
- It launches `yt-dlp.exe` directly with an argument array, and passes the URL after `--` so it can never be parsed as an option.
- yt-dlp runs with `--ignore-config`, so no `yt-dlp.conf` (including one a download might write into the output folder, which yt-dlp would otherwise load as its "home" config) can add options such as `--exec`. Personal yt-dlp config files are therefore not used by this extension.
- Titles and file paths come back from yt-dlp JSON-encoded, so page-controlled titles cannot forge host markers.
- Closing the browser or reloading the extension stops the job's yt-dlp process tree instead of leaving it running unattended.
- Preset IDs are allowlisted.
- Arbitrary custom `yt-dlp` arguments are not supported. Settings that reach the command line (cookie browser, update channel, JavaScript runtime) are checked against fixed allowlists.
- Only `http://` and `https://` URLs are accepted.
- Direct `file:`, `javascript:`, `data:`, `chrome:`, and `blob:` URLs are rejected.
- Instagram presets additionally require an `instagram.com` host.
- `yt-dlp.exe` or its containing folder, ffmpeg, the download folder, and an optional cookies.txt must be absolute local paths.
- JavaScript runtime auto-detection checks PATH and common Windows install folders. Manual runtime paths for `--js-runtimes` must be absolute local paths and must already exist.
- Download output goes only under the configured download root.
- The update action can only run the configured `yt-dlp.exe` with `-U`, or `--update-to` one of `stable`, `nightly`, `master`. It is stopped after 5 minutes.
- Automatic updates are not started while downloads are active.
- Cancel stops only the job's own yt-dlp process tree, with `taskkill /T /F` launched from the Windows System32 folder.
- `Show in folder` and `Show log file` only open files or folders under the configured download folder.
- Job logs are pruned after 30 days. Only files named like this tool's own job logs are ever deleted.
- Post-download cleanup only removes matching yt-dlp temp artifacts next to the job's output files. It does not clear browser caches, global yt-dlp caches, or arbitrary files under the download root.
- Extension pages use a restrictive MV3 content security policy: local scripts only, no plugins, no framing.
- The service worker ignores runtime messages that do not come from the extension's own pages, and forwards only host-relevant settings to the native host.
- The `activeTab` permission is used only by the popup's Download button to read the current tab URL after the user clicks the extension button.
- The only network request the extension makes itself is an optional daily check of this repository's latest GitHub release. It sends no user data.

## Native Messaging Host

Chrome will only start the native host for extension IDs listed in the host manifest `allowed_origins`. The checked-in template is `native-host/com.ytdlp_right_click.native_host.template.json`; the real `native-host/com.ytdlp_right_click.native_host.json` is generated during install because it must contain the user's absolute executable path and the actual extension ID.

The install script writes a user-level registry entry under:

```text
HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ytdlp_right_click.native_host
```

Do not add wildcard origins or unrelated extension IDs to the host manifest. Do not publish a generated manifest from a developer machine.

For public releases, embed the Chrome Web Store extension ID into `scripts/install-native-host.ps1` before packaging the native host installer. Source builds can still prompt for the unpacked extension ID.

## Cookies

Cookie mode defaults to `Auto`. In auto mode, the host first runs `yt-dlp` without cookies. If yt-dlp reports that login or cookies are required, the host retries once with one of:

```text
--cookies-from-browser <chrome|edge|firefox|brave|opera|vivaldi|chromium|whale>
--cookies <absolute path to cookies.txt>
```

Instagram presets start with cookies in Auto mode, and the stories backup refuses to run when cookies are set to `Never`. `Never` and `Always` modes are available in settings.

A cookies.txt file contains live session tokens; keep it private. yt-dlp may write refreshed cookies back to it.

## Logs

The host writes per-job logs under the configured download folder in `_yt-dlp-right-click-logs`. Logs contain process output but not a reconstructed command line. The cookie mode and source (a browser name, or just "cookies.txt") are recorded, never cookie values or the cookies.txt path.

## Expected Limits

This project does not bypass DRM, paywalls, or site restrictions. Final site support depends on `yt-dlp`.

## Updates

Manual and automatic daily updates run one of:

```text
yt-dlp.exe -U
yt-dlp.exe --update-to stable|nightly|master
```

This is intentionally a fixed native-host action, not a custom argument field. If `yt-dlp` is installed through pip, winget, scoop, Chocolatey, or another package manager, that manager may reject or override the update; turn automatic updates off in that setup.
