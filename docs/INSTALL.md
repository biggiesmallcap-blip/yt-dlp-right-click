# Install

These instructions target Google Chrome on Windows.

## Public Build Install

For public releases, users should install in this order:

1. Install the Chrome extension.
2. Download the Windows native host package.
3. Double-click `install-native-host.cmd`.
4. Open the extension Settings page and click `Test native host and settings`.

If the release has a published Chrome Web Store extension ID embedded, users do not need to copy an ID or run a PowerShell command manually. Source and GitHub ZIP builds prompt for the unpacked extension ID when needed.

## Source Build Install

These steps are for development or unpacked source builds.

### 1. Build the Native Host

From the project root:

```powershell
cd native-host
cargo build --release
cd ..
```

The host executable will be:

```text
native-host\target\release\ytdlp_native_host.exe
```

### 2. Load the Chrome Extension

1. Open Chrome.
2. Go to `chrome://extensions`.
3. Enable `Developer mode`.
4. Click `Load unpacked`.
5. Select the project `extension` folder.
6. Open the extension settings page and copy the displayed Extension ID.

### 3. Register the Native Host

Easiest source-build path: double-click:

```text
scripts\install-native-host.cmd
```

Paste the Extension ID when prompted.

Alternative command-line path from the project root:

```powershell
.\scripts\install-native-host.ps1 -ExtensionId "<your-extension-id>"
```

If the popup shows `Specified native messaging host not found`, this step has not been run successfully for the loaded extension ID. Copy the ID from `chrome://extensions` and rerun the command.

If the popup shows `Access to the specified native messaging host is forbidden`, the native host was registered for a different extension ID. Rerun the same command with the exact current ID.

The script writes:

- `native-host\com.ytdlp_right_click.native_host.json`
- `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ytdlp_right_click.native_host`

No administrator privileges are required for the user-level registration.

### 4. Configure the Extension

Open the extension options page and set:

- `yt-dlp.exe path or folder`, for example `C:\Tools\yt-dlp\yt-dlp.exe` or `C:\Tools\yt-dlp`
- `ffmpeg path or folder`, for example `C:\Tools\ffmpeg\bin`
- `download folder`, for example `D:\Downloads\yt-dlp`
- `cookie mode`, normally leave this as `Auto`
- `JavaScript runtime`, normally leave this as `Auto`
- `popup display`, normally leave this as `Simple downloader`
- `automatic yt-dlp update`, optional; leave off if another package manager owns yt-dlp

Click `Test native host and settings`.

For YouTube JavaScript runtime warnings, install a supported runtime such as Node.js or Deno once. Leave the extension setting on `Auto`; the native host checks PATH and common Windows install folders. The manual runtime path is only for unusual installs.

### 5. Use It

Right-click a page, link, selected URL, video, image, or audio element and choose a `yt-dlp` preset.

If a site blocks or hides the right-click target, click the extension button, choose a preset, then click `Download this site`.

The extension badge shows the number of running jobs. Click the extension icon to view live output and recent logs.

## Uninstall

From the project root:

```powershell
.\scripts\uninstall-native-host.ps1
```

Then remove the unpacked extension from `chrome://extensions`.
