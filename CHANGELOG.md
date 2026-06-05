# Changelog

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
