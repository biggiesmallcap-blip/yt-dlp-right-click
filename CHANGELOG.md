# Changelog

## [1.2.0] - 2026-10-15

### Added

- One-step install: a single `yt-dlp-right-click-v1.2.0-windows.zip` with `Setup.cmd`. Setup asks where to install (default `%LOCALAPPDATA%\yt-dlp-right-click`, any user-writable folder allowed) and where downloads go, with Enter keeping the default and **B** opening a folder picker. It remembers the install location for updates and Uninstall.
- Setup finds existing yt-dlp and ffmpeg (PATH, previous install, common folders on every drive), shows their versions, and offers to reuse them, download a fresh copy, browse to another copy, or skip. Very old ffmpeg builds default to a fresh download. Downloads come from official GitHub releases and are verified against their SHA-256 checksums; Deno is only offered when neither Deno nor Node.js is installed.
- ffmpeg downloads use the "shared" build: 85 MB download and 186 MB installed instead of 191 MB and 338 MB.
- Registers the helper for Chrome and Edge and opens `chrome://extensions` with the folder path already on the clipboard.
- `Uninstall.cmd` removes the helper registration and the install folder (wherever it is); downloads and tools Setup only pointed at are kept.
- Settings has **Open download folder** and **Open install folder** buttons, and Check setup warns about very old ffmpeg builds.
- The extension fills in empty yt-dlp, ffmpeg, and download folder settings from the helper, and opens Settings on first install.
- Settings opens with a Status box that says "Ready" or exactly what to do next.

### Security

- Releases are now built by GitHub Actions from the tagged source, with a signed build-provenance attestation (`gh attestation verify`), instead of on a personal machine.
- Each release ships a `SHA256SUMS.txt` for the zip.

### Changed

- The extension has a fixed ID (`kndejcjfodoflkabelfaeiegiaofgjgm`, set by the manifest `key`), so it no longer changes when loaded from a different folder and nobody has to copy an ID into the installer. Upgrading from 1.0/1.1 means loading it once as a new extension.
- Error messages talk about "the helper program" and point to `Setup.cmd` instead of command lines.
- The release is one zip instead of separate extension and native host zips.

### Fixed

- "The native host is installed for a different extension ID" after loading the extension from a new folder.
- The popup said "Finish setup: add the yt-dlp location" when the real problem was a missing helper program.

## [1.1.0] - 2026-10-15

### Added

- Live progress in the popup: a segmented meter with percent, size, speed, time left, and playlist item counts.
- Cancel button for running downloads. The native host stops yt-dlp and its ffmpeg children; partial files are kept so Resume continues them.
- Real media titles on job rows, "Show in folder" that selects the saved file in Explorer, "Show log file", "Copy link", and per-job "Remove".
- "Download again as ..." at the top of the right-click menu, repeating the last preset.
- Instagram presets (only shown on instagram.com): post/reel/carousel, and an incremental stories and highlights backup that skips items it already saved.
- MP3 320 kbps and "Original audio, no re-encode" presets. Audio files can be tagged with title, artist, and cover art (on by default).
- Desktop notifications when a download finishes or fails; clicking one opens the folder.
- Cookie browser choice (Chrome, Edge, Firefox, Brave, and more) and a cookies.txt option, with a clear hint when Chromium cookie encryption blocks yt-dlp.
- yt-dlp release channel setting (stable, nightly, master) and "Check setup", which reports native host, yt-dlp, and ffmpeg versions and warns when yt-dlp is stale.
- Extension update notice from GitHub releases, and a native host version check that flags an outdated host.
- JavaScript unit tests for the shared extension helpers, run in CI.

### Changed

- Redesigned popup and settings page. Settings save automatically.
- Automatic yt-dlp updates are on by default and run from an hourly alarm (at most once a day, never during downloads) instead of only at browser startup. Existing installs are switched on once by the settings migration.
- Job logs older than 30 days (or beyond 300 files) are deleted automatically.
- The "yt-dlp default" JavaScript runtime option now really leaves runtime selection to yt-dlp instead of auto-detecting.
- Right-clicking a video on a known media site (YouTube, SoundCloud, Instagram, and others) now downloads the page, not a blob or CDN chunk; embedded YouTube players use the embed URL.

### Fixed

- Instagram menu items failed with "Unsupported preset" because the native host had no matching presets. The Reels/profile backup item was removed because yt-dlp marks its Instagram profile extractor as broken.
- No progress was ever shown: `--print` silently put yt-dlp in quiet mode.
- Paths showed the Windows `\\?\` prefix in the popup, logs, and arguments passed to yt-dlp.
- Concurrent downloads could overwrite each other's job state, and leave jobs stuck as running, because job updates were unsynchronised read-modify-writes.
- Jobs stayed "running" forever if the native host exited without a final message, or if the extension restarted mid-download.
- A broken stdout pipe (Chrome closed) aborted the host before it waited for yt-dlp, skipping cleanup and the final log entry.
- `yt-dlp -U` could hang the extension indefinitely; helper commands now time out.
- Downloads into a folder whose path is longer than about 260 characters failed to start ("The directory name is invalid"), because yt-dlp was launched with the output folder as its working directory.
- A failed conversion (for example MP3 extraction) could be reported as "complete" because the leftover intermediate file was taken as the result. Only files yt-dlp confirms as final now count.
- Two yt-dlp self-updates could run at once (install, settings migration, and the alarm firing together); updates are now serialised and downloads wait for an update in progress.
- When yt-dlp is too old for the options used, the job now says to update it instead of only showing "no such option".
- Instagram posts in Auto cookie mode failed outright when the browser's cookies could not be read; they now retry without cookies.
- Right-clicking an image on a known media site (X, Reddit, Instagram) sent the page instead of the image, so photos failed to download.
- Temp-file cleanup after one download could delete a concurrent download's partial files when its title started with the same text ("Episode 1" vs "Episode 10").
- Closing the browser or reloading the extension left yt-dlp running invisibly with no log or cancel; the host now stops it, and Retry resumes from the partial file.
- Cancelling a download that was waiting for a yt-dlp update did not stop it from starting.
- The popup kept reporting "native host not installed" for up to 10 minutes after it was fixed.
- Failed automatic updates no longer pile up as entries in the downloads list; the popup shows one notice instead. "Update now" during downloads no longer reports a failure.
- Live progress is stored separately from the job list, so it no longer rewrites every job's history twice a second.
- Popup and settings changes are saved through the service worker, so they cannot overwrite each other.
- Download folders deep enough to exceed the Windows path limit keep working through the `\\?\` form.

### Security

- yt-dlp now runs with `--ignore-config`. Previously a download could save a file named `yt-dlp.conf` into the output folder, which yt-dlp then loaded as configuration for later jobs, so a malicious link could plant options such as `--exec` (present since 1.0.0).
- Titles and file paths are printed as JSON, so a multi-line title cannot forge the host's progress or file markers.
- URLs are passed after `--`, so nothing in a URL can be parsed as a yt-dlp option.
- Cookie browser, update channel, and runtime names are allowlisted in the host; cookies.txt must be an absolute path to an existing, reasonably sized file.
- Instagram presets verify the host is instagram.com before running.
- The service worker only accepts messages from the extension's own pages, and only forwards host-relevant settings to the native host.
- `taskkill` and `explorer` are launched by absolute Windows path.

## [1.0.0] - 2026-06-05

### Added

- Initial GitHub release of the Chrome MV3 extension and Windows native messaging host.
- Right-click and popup download flows for page, link, selected text, image, video, and audio URLs.
- Preset-only native execution for video, audio, file, and explicit playlist downloads.
- Cookie retry modes, JavaScript runtime detection, manual yt-dlp update, job history, retry, and open-folder actions.
- Release packaging script, CI workflow, install guide, and security boundary documentation.

### Security

- Native host builds yt-dlp commands from hardcoded presets and launches without a shell.
- Native host rejects non-http(s) URLs, raw whitespace/control characters, unsupported job IDs, and output paths outside the configured target folder.
- Open-folder requests are constrained to the configured download root.
- Generated native host manifests remain ignored and excluded from release artifacts.
