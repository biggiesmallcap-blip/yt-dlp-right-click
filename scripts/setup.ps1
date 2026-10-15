# One-step installer for yt-dlp Right Click (release package).
#
# Asks where to install and where downloads go (Enter keeps the default), finds
# or downloads yt-dlp, ffmpeg, and Deno (each download checked against its
# official SHA-256 checksum), registers the helper with Chrome and Edge for this
# user, writes default settings, and opens Chrome's extensions page with the
# extension folder path on the clipboard. No administrator rights needed.
param(
    [string]$InstallDir = "",
    [string]$DownloadDir = "",
    [switch]$NoDownload,
    # Accept every default without asking (unattended install).
    [switch]$Yes,
    # For testing: install files and tools but leave the browser registration,
    # remembered install location, clipboard, and Chrome alone.
    [switch]$SkipBrowserSteps
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is very slow with the progress bar on
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# Fixed by the "key" in extension/manifest.json, so it is the same whatever
# folder the extension is loaded from.
$ExtensionId = "kndejcjfodoflkabelfaeiegiaofgjgm"
$HostName = "com.ytdlp_right_click.native_host"
$StateKey = "HKCU:\Software\yt-dlp-right-click"
$DefaultInstallDir = Join-Path $env:LOCALAPPDATA "yt-dlp-right-click"

$YtDlpUrl = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
$YtDlpSums = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"
$FfmpegName = "ffmpeg-master-latest-win64-gpl-shared.zip"
$FfmpegUrl = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/$FfmpegName"
$FfmpegSums = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256"
$DenoName = "deno-x86_64-pc-windows-msvc.zip"
$DenoUrl = "https://github.com/denoland/deno/releases/latest/download/$DenoName"
$DenoSums = "$DenoUrl.sha256sum"

function Step([string]$Text) { Write-Host ""; Write-Host "==> $Text" -ForegroundColor Cyan }
function Ok([string]$Text) { Write-Host "    $Text" -ForegroundColor Green }
function Note([string]$Text) { Write-Host "    $Text" }
function Warn([string]$Text) { Write-Host "    $Text" -ForegroundColor Yellow }

# --- Prompts -------------------------------------------------------------------

function Read-Choice([string]$Prompt, [string[]]$Allowed, [string]$Default) {
    if ($Yes) { return $Default }
    while ($true) {
        $answer = (Read-Host "    $Prompt").Trim()
        if (-not $answer) { return $Default }
        $letter = $answer.Substring(0, 1).ToUpperInvariant()
        if ($answer.Length -eq 1 -and $Allowed -contains $letter) { return $letter }
        if ($Allowed -contains "PATH") { return $answer }
        Warn "Please type one of: $($Allowed -join ', '), or press Enter."
    }
}

function Show-FolderPicker([string]$Description, [string]$Start) {
    Add-Type -AssemblyName System.Windows.Forms
    $owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true }
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = $Description
    $dialog.ShowNewFolderButton = $true
    if ($Start -and (Test-Path -LiteralPath $Start)) { $dialog.SelectedPath = $Start }
    try {
        if ($dialog.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) { return $dialog.SelectedPath }
        return $null
    }
    finally { $owner.Dispose() }
}

function Show-FilePicker([string]$Title, [string]$FileName) {
    Add-Type -AssemblyName System.Windows.Forms
    $owner = New-Object System.Windows.Forms.Form -Property @{ TopMost = $true }
    $dialog = New-Object System.Windows.Forms.OpenFileDialog
    $dialog.Title = $Title
    $dialog.Filter = "$FileName|$FileName"
    try {
        if ($dialog.ShowDialog($owner) -eq [System.Windows.Forms.DialogResult]::OK) { return $dialog.FileName }
        return $null
    }
    finally { $owner.Dispose() }
}

# Asks for a folder: Enter keeps the default, B opens a picker, or a typed path.
function Read-Folder([string]$Question, [string]$Default) {
    Write-Host ""
    Write-Host "    $Question" -ForegroundColor White
    Note "  Enter = $Default"
    Note "  B     = browse for a folder"
    Note "  or paste a folder path"
    while ($true) {
        $answer = Read-Choice "Choice" @("B", "PATH") ""
        if (-not $answer) { return $Default }
        if ($answer -eq "B") {
            $picked = Show-FolderPicker $Question $Default
            if ($picked) { return $picked }
            Warn "No folder picked."
            continue
        }
        $path = $answer.Trim('"', ' ')
        if ([IO.Path]::IsPathRooted($path) -and $path -match '^[A-Za-z]:\\|^\\\\') { return $path }
        Warn "That doesn't look like a full folder path (for example D:\Videos)."
    }
}

# --- Tools ---------------------------------------------------------------------

function Find-OnPath([string]$Name) {
    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($command) { return $command.Source }
    return $null
}

function Get-FixedDrives {
    [IO.DriveInfo]::GetDrives() | Where-Object { $_.DriveType -eq "Fixed" -and $_.IsReady } | ForEach-Object { $_.RootDirectory.FullName }
}

function Get-FirstLine([string]$Exe, [string]$Argument) {
    try {
        $line = & $Exe $Argument 2>$null | Select-Object -First 1
        return [string]$line
    }
    catch { return "" }
}

function Get-FfmpegMajor([string]$VersionLine) {
    if ($VersionLine -match 'version n?(\d+)\.') { return [int]$Matches[1] }
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
        if (Test-Path -LiteralPath $TargetDir) { Remove-Item -LiteralPath $TargetDir -Recurse -Force }
        New-Item -ItemType Directory -Force -Path $TargetDir | Out-Null
        # Only the named programs and the DLLs they need (skips ffplay etc.).
        Get-ChildItem -LiteralPath $exe.DirectoryName -File |
            Where-Object { $_.Name -in $ExeNames -or $_.Extension -eq ".dll" } |
            Copy-Item -Destination $TargetDir -Force
    }
    finally {
        Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $Zip -Force -ErrorAction SilentlyContinue
    }
}

<#
  Shows what was found for one tool and asks what to do.
  Returns "use", "download", "skip", or a path picked with Browse.
#>
function Read-ToolChoice([string]$Label, [string]$Found, [string]$Detail, [bool]$PreferDownload, [string]$Size, [string]$ExeName) {
    Write-Host ""
    Write-Host "    $Label" -ForegroundColor White
    if ($Found) {
        Note "  Found: $Found$(if ($Detail) { "  ($Detail)" })"
        if ($PreferDownload) {
            Note "  Enter = download a fresh copy ($Size)   U = use the one found   B = browse   S = skip"
            $default = "D"; $allowed = @("U", "D", "B", "S")
        }
        else {
            Note "  Enter = use it   D = download a fresh copy ($Size)   B = browse   S = skip"
            $default = "U"; $allowed = @("U", "D", "B", "S")
        }
    }
    else {
        Note "  Not found on this PC."
        Note "  Enter = download it ($Size)   B = browse to your own copy   S = skip"
        $default = "D"; $allowed = @("D", "B", "S")
    }
    if ($NoDownload -and $default -eq "D") { $default = if ($Found) { "U" } else { "S" } }

    while ($true) {
        switch (Read-Choice "Choice" $allowed $default) {
            "U" { return "use" }
            "D" { return "download" }
            "S" { return "skip" }
            "B" {
                $picked = Show-FilePicker "Find $ExeName" $ExeName
                if ($picked) { return $picked }
                Warn "Nothing picked."
            }
        }
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

Write-Host "yt-dlp Right Click setup" -ForegroundColor White
Note "Installs for your Windows user only. No administrator rights needed."
Note "Press Enter at any question to keep the suggested answer."

# --- 1. Where to install ----------------------------------------------------------
$previousInstall = ""
if (Test-Path $StateKey) { $previousInstall = [string](Get-Item $StateKey).GetValue("InstallDir") }
if (-not $InstallDir) {
    $suggested = if ($previousInstall) { $previousInstall } else { $DefaultInstallDir }
    $InstallDir = Read-Folder "Where should yt-dlp Right Click be installed? (about 1 MB, plus any tools it downloads)" $suggested
}
$InstallDir = [IO.Path]::GetFullPath($InstallDir)
foreach ($protected in @($env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:SystemRoot)) {
    if ($protected -and $InstallDir.StartsWith($protected, [StringComparison]::OrdinalIgnoreCase)) {
        throw "$protected needs administrator rights. Pick a folder you own, like the default or a folder on another drive."
    }
}

$ExtensionDir = Join-Path $InstallDir "extension"
$HostDir = Join-Path $InstallDir "host"
$ToolsDir = Join-Path $InstallDir "tools"
$HostExe = Join-Path $HostDir "ytdlp_native_host.exe"
$ConfigPath = Join-Path $HostDir "config.json"

$previous = @{}
foreach ($configFile in @($ConfigPath, $(if ($previousInstall) { Join-Path $previousInstall "host\config.json" }))) {
    if ($configFile -and (Test-Path -LiteralPath $configFile)) {
        try { $previous = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json; break } catch { }
    }
}

# --- 2. Where downloads go ----------------------------------------------------------
if (-not $DownloadDir) {
    $suggested = if ($previous.downloadDir) { [string]$previous.downloadDir } else { Join-Path $env:USERPROFILE "Downloads\yt-dlp" }
    $DownloadDir = Read-Folder "Where should your downloaded videos and music be saved?" $suggested
}

# --- 3. Tools ---------------------------------------------------------------------
Step "Looking for yt-dlp, ffmpeg, and a JavaScript runtime"
$drives = @(Get-FixedDrives)

# yt-dlp
$ytCandidates = @((Join-Path $ToolsDir "yt-dlp.exe"), [string]$previous.ytDlpPath, (Find-OnPath "yt-dlp.exe"))
$ytCandidates += $drives | ForEach-Object { Join-Path $_ "yt-dlp\yt-dlp.exe"; Join-Path $_ "Tools\yt-dlp\yt-dlp.exe"; Join-Path $_ "Tools\yt-dlp.exe" }
$ytFound = $ytCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
$ytDetail = ""; $ytManaged = $false
if ($ytFound) {
    $ytDetail = "version " + (Get-FirstLine $ytFound "--version")
    if ($ytFound -match '\\WinGet\\|\\scoop\\|\\Scripts\\|\\pipx\\|\\chocolatey\\') {
        $ytManaged = $true
        $ytDetail += ", managed by a package manager, so automatic updates can't update it"
    }
}
$ytChoice = Read-ToolChoice "yt-dlp (does the downloading)" $ytFound $ytDetail $ytManaged "17 MB" "yt-dlp.exe"

# ffmpeg: prefer a modern copy if several exist.
$ffCandidates = @((Join-Path $ToolsDir "ffmpeg\bin\ffmpeg.exe"), [string]$previous.ffmpegPath, (Find-OnPath "ffmpeg.exe"))
$ffCandidates += $drives | ForEach-Object { Join-Path $_ "ffmpeg\bin\ffmpeg.exe"; Join-Path $_ "Tools\ffmpeg\bin\ffmpeg.exe"; Join-Path $_ "yt-dlp\ffmpeg.exe" }
$ffFound = ""; $ffDetail = ""; $ffOld = $false
foreach ($candidate in ($ffCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -Unique)) {
    $line = Get-FirstLine $candidate "-version"
    $major = Get-FfmpegMajor $line
    $isOld = $null -ne $major -and $major -lt 5
    if (-not $ffFound -or ($ffOld -and -not $isOld)) {
        $ffFound = $candidate; $ffOld = $isOld
        $ffDetail = ($line -replace '^ffmpeg version (\S+).*$', 'version $1')
        if ($isOld) { $ffDetail += ", very old and fails on some formats" }
    }
}
$ffChoice = Read-ToolChoice "ffmpeg (joins video and audio, converts to MP3)" $ffFound $ffDetail $ffOld "85 MB download, 186 MB installed" "ffmpeg.exe"

# JavaScript runtime: only asked about when none is installed.
$runtime = @("deno.exe", "node.exe") | ForEach-Object { Find-OnPath $_ } | Where-Object { $_ } | Select-Object -First 1
if (-not $runtime) {
    $runtime = @(
        (Join-Path $ToolsDir "deno\deno.exe"),
        (Join-Path $env:USERPROFILE ".deno\bin\deno.exe"),
        (Join-Path $env:ProgramFiles "nodejs\node.exe")
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
$denoChoice = "skip"
if ($runtime) {
    Write-Host ""
    Ok "JavaScript runtime for YouTube: $runtime"
}
else {
    Write-Host ""
    Write-Host "    Deno (YouTube needs a JavaScript runtime; none found)" -ForegroundColor White
    Note "  Enter = download it (41 MB download, 93 MB installed)   S = skip"
    $default = if ($NoDownload) { "S" } else { "D" }
    if ((Read-Choice "Choice" @("D", "S") $default) -eq "D") { $denoChoice = "download" }
}

# --- 4. Install ---------------------------------------------------------------------
Step "Installing to $InstallDir"
Get-Process ytdlp_native_host -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $InstallDir, $HostDir, $ToolsDir | Out-Null

$sameFolder = [IO.Path]::GetFullPath($SourceExtension).TrimEnd('\') -ieq $ExtensionDir.TrimEnd('\')
if (-not $sameFolder) {
    if (Test-Path -LiteralPath $ExtensionDir) { Remove-Item -LiteralPath $ExtensionDir -Recurse -Force }
    Copy-Item -LiteralPath $SourceExtension -Destination $ExtensionDir -Recurse
    Copy-Item -LiteralPath $SourceHost -Destination $HostExe -Force
}
Ok "Extension and helper program copied."

$YtDlp = ""
switch ($ytChoice) {
    "use" { $YtDlp = $ytFound }
    "skip" { }
    "download" {
        Note "Downloading yt-dlp..."
        $YtDlp = Join-Path $ToolsDir "yt-dlp.exe"
        Save-Verified $YtDlpUrl $YtDlpSums "yt-dlp.exe" $YtDlp
        Ok "yt-dlp downloaded and verified."
    }
    default { $YtDlp = $ytChoice }
}

$Ffmpeg = ""
switch ($ffChoice) {
    "use" { $Ffmpeg = $ffFound }
    "skip" { }
    "download" {
        Note "Downloading ffmpeg..."
        $zip = Join-Path $ToolsDir "ffmpeg.zip"
        Save-Verified $FfmpegUrl $FfmpegSums $FfmpegName $zip
        Expand-Tool $zip @("ffmpeg.exe", "ffprobe.exe") (Join-Path $ToolsDir "ffmpeg\bin")
        $Ffmpeg = Join-Path $ToolsDir "ffmpeg\bin\ffmpeg.exe"
        Ok "ffmpeg downloaded and verified."
    }
    default { $Ffmpeg = $ffChoice }
}

if ($denoChoice -eq "download") {
    Note "Downloading Deno..."
    $zip = Join-Path $ToolsDir "deno.zip"
    Save-Verified $DenoUrl $DenoSums $DenoName $zip
    Expand-Tool $zip @("deno.exe") (Join-Path $ToolsDir "deno")
    Ok "Deno downloaded and verified."
}
Get-ChildItem -LiteralPath $InstallDir -Recurse -File | Unblock-File -ErrorAction SilentlyContinue

if (-not $YtDlp) { Warn "No yt-dlp chosen. Set it later in the extension's Settings." }
if (-not $Ffmpeg) { Warn "No ffmpeg chosen. MP4 and MP3 formats need it; set it later in Settings." }

New-Item -ItemType Directory -Force -Path $DownloadDir | Out-Null
$config = [ordered]@{ ytDlpPath = $YtDlp; ffmpegPath = $Ffmpeg; downloadDir = $DownloadDir }
[IO.File]::WriteAllText($ConfigPath, ($config | ConvertTo-Json), (New-Object Text.UTF8Encoding $false))
Ok "Downloads will be saved to $DownloadDir"

# --- 5. Register the helper with Chrome and Edge -----------------------------------
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
# Remember the location so updates and Uninstall find it.
New-Item -Path $StateKey -Force | Out-Null
Set-ItemProperty -Path $StateKey -Name "InstallDir" -Value $InstallDir
Ok "Done (Chrome and Edge)."

# --- 6. Hand over to Chrome ---------------------------------------------------------
$moved = $previousInstall -and ($previousInstall.TrimEnd('\') -ine $InstallDir.TrimEnd('\'))
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
if ($moved) {
    Warn "You installed to a new location. Remove the old yt-dlp Right Click card in Chrome first;"
    Warn "the old folder ($previousInstall) can be deleted afterwards."
} else {
    Note "Already installed here before? Just click the reload arrow on the yt-dlp Right Click card."
}
Write-Host ""
Ok "Settings fill themselves in. Right-click any video and choose yt-dlp."
