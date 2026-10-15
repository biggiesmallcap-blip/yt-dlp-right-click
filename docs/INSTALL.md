# Install

## For users

1. Download `yt-dlp-right-click-v<version>-windows.zip` from the [latest release](https://github.com/biggiesmallcap-blip/yt-dlp-right-click/releases/latest) and unzip it.
2. Double-click `Setup.cmd`. If Windows says "Windows protected your PC", click **More info**, then **Run anyway**. Press Enter when it offers to download missing tools.
3. In the Chrome tab Setup opens: turn on **Developer mode**, click **Load unpacked**, press **Ctrl+V**, then **Select Folder**.

The extension's Settings page then shows **Ready**. That's the whole install.

What Setup does:

- Asks where to install (default `%LOCALAPPDATA%\yt-dlp-right-click`; any folder you can write to, not Program Files) and where downloads go (default `Downloads\yt-dlp`). Enter keeps the default, **B** opens a folder picker, or paste a path.
- Copies the extension and the helper program into the install folder, and remembers the location in `HKCU\Software\yt-dlp-right-click` for updates and Uninstall.
- Looks for yt-dlp and ffmpeg (previous install, PATH, and folders like `X:\yt-dlp` and `X:\Tools` on every drive), shows their versions, and offers to reuse them. Otherwise it offers to download them into the install folder's `tools`, checking each against its official SHA-256 checksum. Deno is offered only if neither Deno nor Node.js is installed.
- Registers the helper for Chrome and Edge (per user, in `HKCU`).
- Writes suggested settings (tool locations and download folder) that the extension picks up automatically.

Options for `setup.ps1`: `-InstallDir <folder>` and `-DownloadDir <folder>` to skip those questions, `-NoDownload` to never download tools, and `-Yes` to accept every suggestion without asking.

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
