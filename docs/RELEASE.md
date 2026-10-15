# Release Process

These steps prepare the GitHub release artifacts.

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
   Get-ChildItem extension\*.js | ForEach-Object { node --check $_.FullName }
   node --test tests/shared.test.mjs
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
.\scripts\package-release.ps1 -Version 1.2.0
```

The script creates `dist\1.2.0\yt-dlp-right-click-v1.2.0-windows.zip`: one folder with `Setup.cmd`, `setup.ps1`, `Uninstall.cmd`, `uninstall.ps1`, a short `README.txt`, `SECURITY.md`, `extension\`, and `host\ytdlp_native_host.exe`. It never contains a generated native host manifest or `config.json`.

## GitHub Release

Releases are built by `.github/workflows/release.yml`, never uploaded from a local machine:

1. Bump the version in `extension/manifest.json` and `native-host/Cargo.toml`, add a `CHANGELOG.md` section, and merge to `main`.
2. Tag the merge commit and push the tag:

   ```powershell
   git tag v1.2.0
   git push origin v1.2.0
   ```

3. The workflow runs the tests, builds the zip and `SHA256SUMS.txt`, attests their provenance, and publishes the release with notes taken from the changelog.
4. Scan the published zip on VirusTotal and add the report link to the release notes.

The local `package-release.ps1` run above is for testing the package only.

## Manual Smoke Test

1. Unzip the release ZIP and run `Setup.cmd`.
2. Load `%LOCALAPPDATA%\yt-dlp-right-click\extension` as an unpacked extension.
3. Open Settings: the Status box should say Ready and the tool paths should be filled in.
4. Start one `Best single file` job against a known public URL.
5. Confirm the popup shows completion and `Show in folder` opens the download folder.
