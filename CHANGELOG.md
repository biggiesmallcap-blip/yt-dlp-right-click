# Changelog

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

### Security

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
