# One-step installer for yt-dlp Right Click (release package).
#
# Copies the extension and helper into a permanent folder, downloads any
# missing tools (yt-dlp, ffmpeg, Deno) with checksum verification, registers
# the helper with Chrome and Edge for this user, writes default settings, and
# opens Chrome's extensions page with the folder path on the clipboard.
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "yt-dlp-right-click"),
    [string]$DownloadDir = "",
    [switch]$NoDownload,
    [switch]$Yes,
    # For testing: install files and tools but leave the browser registration,
    # clipboard, and Chrome alone.
    [switch]$SkipBrowserSteps
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is very slow with the progress bar on
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# Fixed by the "key" in extension/manifest.json, so it is the same whatever
# folder the extension is loaded from.
$ExtensionId = "kndejcjfodoflkabelfaeiegiaofgjgm"
$HostName = "com.ytdlp_right_click.native_host"

function Step([string]$Text) { Write-Host ""; Write-Host "==> $Text" -ForegroundColor Cyan }
function Ok([string]$Text) { Write-Host "    $Text" -ForegroundColor Green }
function Note([string]$Text) { Write-Host "    $Text" }
function Warn([string]$Text) { Write-Host "    $Text" -ForegroundColor Yellow }

function Find-OnPath([string]$Name) {
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { return $command.Source }
    return $null
}

function Get-ExpectedHash([string]$SumsUrl, [string]$FileName) {
    $text = (Invoke-WebRequest -Uri $SumsUrl -UseBasicParsing).Content
    if ($text -is [byte[]]) { $text = [Text.Encoding]::UTF8.GetString($text) }
    # "<hash>  <name>" lists (yt-dlp, FFmpeg-Builds)
    foreach ($line in ($text -split "`r?`n")) {
        $parts = $line.Trim() -split '\s+'
        if ($parts.Count -ge 2 -and $parts[-1].TrimStart('*') -eq $FileName -and $parts[0] -match '^[0-9a-fA-F]{64}$') {
            return $parts[0].ToUpperInvariant()
        }
    }
    # Single-file checksum in any layout (Deno uses Get-FileHash output)
    $hashes = [regex]::Matches($text, '\b[0-9a-fA-F]{64}\b') | ForEach-Object { $_.Value.ToUpperInvariant() } | Select-Object -Unique
    if (@($hashes).Count -eq 1) { return @($hashes)[0] }
    throw "Could not find a checksum for $FileName."
}

function Save-Verified([string]$Url, [string]$SumsUrl, [string]$FileName, [string]$Destination) {
    $expected = Get-ExpectedHash $SumsUrl $FileName
    $partial = "$Destination.download"
    Invoke-WebRequest -Uri $Url -OutFile $partial -UseBasicParsing
    $actual = (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        Remove-Item -LiteralPath $partial -Force
        throw "Checksum mismatch for $FileName. The download was deleted; try again later."
    }
    Move-Item -LiteralPath $partial -Destination $Destination -Force
}

function Expand-Tool([string]$Zip, [string[]]$ExeNames, [string]$TargetDir) {
    $temp = Join-Path ([IO.Path]::GetTempPath()) ("ytrc-" + [guid]::NewGuid())
    try {
        Expand-Archive -LiteralPath $Zip -DestinationPath $temp -Force
        $exe = Get-ChildItem -LiteralPath $temp -Recurse -Filter $ExeNames[0] | Select-Object -First 1
        if (-not $exe) { throw "$($ExeNames[0]) was not found in the download." }
        New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
        # Only the named programs and any DLLs they need (skips ffplay etc.).
        Get-ChildItem -LiteralPath $exe.DirectoryName -File |
            Where-Object { $_.Name -in $ExeNames -or $_.Extension -eq ".dll" } |
            Copy-Item -Destination $TargetDir -Force
    }
    finally {
        Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $Zip -Force -ErrorAction SilentlyContinue
    }
}

function Find-Chrome {
    foreach ($root in "HKCU:", "HKLM:") {
        $key = "$root\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\chrome.exe"
        if (Test-Path $key) {
            $path = (Get-Item $key).GetValue("")
            if ($path -and (Test-Path -LiteralPath $path)) { return $path }
        }
    }
    return $null
}

# ---------------------------------------------------------------------------

$Here = $PSScriptRoot
$SourceExtension = Join-Path $Here "extension"
$SourceHost = Join-Path $Here "host\ytdlp_native_host.exe"
if (-not (Test-Path -LiteralPath (Join-Path $SourceExtension "manifest.json")) -or -not (Test-Path -LiteralPath $SourceHost)) {
    throw "Run Setup.cmd from inside the unzipped yt-dlp Right Click folder (it needs the 'extension' and 'host' folders next to it)."
}

$ExtensionDir = Join-Path $InstallDir "extension"
$HostDir = Join-Path $InstallDir "host"
$ToolsDir = Join-Path $InstallDir "tools"
$HostExe = Join-Path $HostDir "ytdlp_native_host.exe"
$ConfigPath = Join-Path $HostDir "config.json"

Write-Host "yt-dlp Right Click setup" -ForegroundColor White
Note "Installs for your Windows user only. No administrator rights needed."

# --- 1. Copy files -----------------------------------------------------------
Step "Installing to $InstallDir"
Get-Process ytdlp_native_host -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $InstallDir, $HostDir, $ToolsDir | Out-Null

$sameFolder = [IO.Path]::GetFullPath($SourceExtension).TrimEnd('\') -ieq [IO.Path]::GetFullPath($ExtensionDir).TrimEnd('\')
if (-not $sameFolder) {
    if (Test-Path -LiteralPath $ExtensionDir) { Remove-Item -LiteralPath $ExtensionDir -Recurse -Force }
    Copy-Item -LiteralPath $SourceExtension -Destination $ExtensionDir -Recurse
    Copy-Item -LiteralPath $SourceHost -Destination $HostExe -Force
}
Get-ChildItem -LiteralPath $InstallDir -Recurse -File | Unblock-File -ErrorAction SilentlyContinue
Ok "Extension and helper program copied."

# --- 2. Tools ----------------------------------------------------------------
Step "Checking for yt-dlp, ffmpeg, and a JavaScript runtime"
$previous = @{}
if (Test-Path -LiteralPath $ConfigPath) {
    try { $previous = Get-Content -LiteralPath $ConfigPath -Raw | ConvertFrom-Json } catch { $previous = @{} }
}

# yt-dlp: prefer our own standalone copy, which the extension can keep updated.
$YtDlp = Join-Path $ToolsDir "yt-dlp.exe"
$needYtDlp = -not (Test-Path -LiteralPath $YtDlp)

$Ffmpeg = Join-Path $ToolsDir "ffmpeg\bin\ffmpeg.exe"
if (-not (Test-Path -LiteralPath $Ffmpeg)) {
    $found = Find-OnPath "ffmpeg.exe"
    if ($found) { $Ffmpeg = $found }
}
$needFfmpeg = -not (Test-Path -LiteralPath $Ffmpeg)

$runtime = @("deno.exe", "node.exe") | ForEach-Object { Find-OnPath $_ } | Where-Object { $_ } | Select-Object -First 1
if (-not $runtime) {
    $runtime = @(
        (Join-Path $ToolsDir "deno\deno.exe"),
        (Join-Path $env:USERPROFILE ".deno\bin\deno.exe"),
        (Join-Path $env:ProgramFiles "nodejs\node.exe")
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
$needRuntime = -not $runtime

if (-not $needYtDlp) { Ok "yt-dlp: $YtDlp" }
if (-not $needFfmpeg) { Ok "ffmpeg: $Ffmpeg" }
if (-not $needRuntime) { Ok "JavaScript runtime: $runtime" }

$missing = @()
if ($needYtDlp) { $missing += "yt-dlp (about 18 MB)" }
if ($needFfmpeg) { $missing += "ffmpeg (about 200 MB)" }
if ($needRuntime) { $missing += "Deno, needed for YouTube (about 45 MB)" }

if ($missing.Count -gt 0) {
    Note ""
    Note "Missing: $($missing -join ', ')."
    $download = -not $NoDownload
    if ($download -and -not $Yes) {
        $answer = Read-Host "    Download them now from their official GitHub releases? [Y/n]"
        $download = $answer -notmatch '^\s*n'
    }

    if ($download) {
        if ($needYtDlp) {
            Note "Downloading yt-dlp..."
            Save-Verified "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" `
                "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS" "yt-dlp.exe" $YtDlp
            Ok "yt-dlp installed and verified."
        }
        if ($needFfmpeg) {
            Note "Downloading ffmpeg (this is the big one)..."
            $zip = Join-Path $ToolsDir "ffmpeg.zip"
            Save-Verified "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip" `
                "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256" "ffmpeg-master-latest-win64-gpl.zip" $zip
            Expand-Tool $zip @("ffmpeg.exe", "ffprobe.exe") (Join-Path $ToolsDir "ffmpeg\bin")
            $Ffmpeg = Join-Path $ToolsDir "ffmpeg\bin\ffmpeg.exe"
            Ok "ffmpeg installed and verified."
        }
        if ($needRuntime) {
            Note "Downloading Deno..."
            $zip = Join-Path $ToolsDir "deno.zip"
            Save-Verified "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip" `
                "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum" "deno-x86_64-pc-windows-msvc.zip" $zip
            Expand-Tool $zip @("deno.exe") (Join-Path $ToolsDir "deno")
            Ok "Deno installed and verified."
        }
    }
    else {
        Warn "Skipped. Fill in the tool locations later in the extension's Settings."
    }
}

# Fall back to a yt-dlp already on PATH if we did not download our own.
if (-not (Test-Path -LiteralPath $YtDlp)) {
    $found = Find-OnPath "yt-dlp.exe"
    $YtDlp = if ($found) { $found } else { "" }
}
if (-not (Test-Path -LiteralPath $Ffmpeg)) { $Ffmpeg = "" }

# --- 3. Download folder and default settings ----------------------------------
if (-not $DownloadDir) {
    $DownloadDir = if ($previous.downloadDir) { [string]$previous.downloadDir } else { Join-Path $env:USERPROFILE "Downloads\yt-dlp" }
}
New-Item -ItemType Directory -Force -Path $DownloadDir | Out-Null

$config = [ordered]@{ ytDlpPath = $YtDlp; ffmpegPath = $Ffmpeg; downloadDir = $DownloadDir }
[IO.File]::WriteAllText($ConfigPath, ($config | ConvertTo-Json), (New-Object Text.UTF8Encoding $false))

Step "Downloads will be saved to $DownloadDir"
Note "You can change this any time in the extension's Settings."

# --- 4. Register the helper with Chrome and Edge -----------------------------
Step "Connecting the helper program to Chrome"
$manifestPath = Join-Path $HostDir "$HostName.json"
$manifest = [ordered]@{
    name            = $HostName
    description     = "yt-dlp Right Click helper"
    path            = $HostExe
    type            = "stdio"
    allowed_origins = @("chrome-extension://$ExtensionId/")
}
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 4), (New-Object Text.UTF8Encoding $false))
if ($SkipBrowserSteps) {
    Ok "Skipped browser registration (-SkipBrowserSteps)."
    return
}
foreach ($browserKey in "HKCU:\Software\Google\Chrome\NativeMessagingHosts", "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts") {
    $key = "$browserKey\$HostName"
    New-Item -Path $key -Force | Out-Null
    Set-Item -Path $key -Value $manifestPath
}
Ok "Done (Chrome and Edge)."

# --- 5. Hand over to Chrome -----------------------------------------------------
Set-Clipboard -Value $ExtensionDir
$chrome = Find-Chrome
if ($chrome) { Start-Process -FilePath $chrome -ArgumentList "chrome://extensions" }

Write-Host ""
Write-Host "Almost done. One last step in Chrome:" -ForegroundColor White
Write-Host ""
if ($chrome) {
    Note "In the Extensions tab that just opened:"
} else {
    Note "Open Chrome and go to chrome://extensions, then:"
}
Note "  1. Turn on 'Developer mode' (the switch at the top right)."
Note "  2. Click 'Load unpacked'."
Note "  3. Press Ctrl+V to paste the folder path (already copied for you), then click 'Select Folder'."
Note "     Folder: $ExtensionDir"
Write-Host ""
Note "Already installed before? Click the reload arrow on the yt-dlp Right Click card instead,"
Note "and remove any older copy that was loaded from a different folder."
Write-Host ""
Ok "Settings fill themselves in. Right-click any video and choose yt-dlp."
