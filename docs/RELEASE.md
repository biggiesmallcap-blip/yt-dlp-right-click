# Release Process

These steps prepare the GitHub v1.0 release artifacts.

## Preflight

1. Confirm versions match:

   ```powershell
   Select-String -Path extension\manifest.json,native-host\Cargo.toml -Pattern '"version"|^version'
   ```

2. Run local checks:

   ```powershell
   cd native-host
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   cd ..
   node --check extension\background.js
   node --check extension\options.js
   node --check extension\popup.js
   python -m json.tool extension\manifest.json > $null
   python -m json.tool native-host\com.ytdlp_right_click.native_host.template.json > $null
   ```

3. Check whether a generated local manifest is present:

   ```powershell
   Test-Path native-host\com.ytdlp_right_click.native_host.json
   ```

   If this prints `True`, leave the file local. The packaging script warns and excludes it from release artifacts.

## Build Artifacts

From the repository root:

```powershell
.\scripts\package-release.ps1 -Version 1.0.0
```

The script creates:

- `dist\1.0.0\yt-dlp-right-click-extension-v1.0.0.zip`
- `dist\1.0.0\yt-dlp-right-click-native-host-windows-v1.0.0.zip`

The native host ZIP contains the executable, installer scripts, uninstall script, README, install guide, security notes, and native host manifest template. It intentionally does not contain a generated `com.ytdlp_right_click.native_host.json`.

## GitHub Release

1. Create tag `v1.0.0`.
2. Use the `CHANGELOG.md` `1.0.0` section as release notes.
3. Attach both ZIP files from `dist\1.0.0`.
4. Mark the release as stable after CI passes.

## Manual Smoke Test

1. Load the extension ZIP contents as an unpacked extension in Chrome.
2. Run the packaged native host installer and provide the extension ID when prompted.
3. Open extension settings and click `Test native host and settings`.
4. Start one `File: Best available` job against a known public URL.
5. Confirm the popup shows completion and `Open folder` opens under the configured download directory.
