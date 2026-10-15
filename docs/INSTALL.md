# Install

## For users

1. Download `yt-dlp-right-click-v<version>-windows.zip` from the [latest release](https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/latest) and unzip it.
2. Double-click `Setup.cmd`. If Windows says "Windows protected your PC", click **More info**, then **Run anyway**. Press Enter when it offers to download missing tools.
3. In the Chrome tab Setup opens: turn on **Developer mode**, click **Load unpacked**, press **Ctrl+V**, then **Select Folder**.

The extension's Settings page then shows **Ready**. That's the whole install.

What Setup does:

- Copies the extension and the helper program to `%LOCALAPPDATA%\yt-dlp-right-click`.
- Downloads yt-dlp, ffmpeg, and Deno into `...\tools` if they are not already on this PC, checking each against its official SHA-256 checksum.
- Registers the helper for Chrome and Edge (per user, in `HKCU`).
- Writes suggested settings (tool locations and `Downloads\yt-dlp`) that the extension picks up automatically.

Options for `setup.ps1`: `-DownloadDir <folder>` to pick the download folder, `-NoDownload` to skip tool downloads, `-Yes` to answer prompts automatically.

### Updating

Run the new version's `Setup.cmd`, then click the reload arrow on the extension's card in `chrome://extensions`.

### Uninstalling

Run `Uninstall.cmd`, then click **Remove** on the extension in `chrome://extensions`. Downloaded media is never deleted.

### If something is wrong

Open the extension's Settings. The Status box at the top says what's missing, and **Check setup** runs yt-dlp and ffmpeg once and reports their versions.

## For developers

1. Build the helper:

   ```powershell
   cd native-host
   cargo build --release
   cd ..
   ```

2. Register it for the extension (no ID needed; it is fixed by the manifest `key`):

   ```powershell
   .\scripts\install-native-host.ps1
   ```

3. In `chrome://extensions`, enable Developer mode and **Load unpacked** the `extension` folder.

The helper suggests tool paths it finds on `PATH`, so a source install usually fills its own settings too. `.\scripts\uninstall-native-host.ps1` removes the registration.
