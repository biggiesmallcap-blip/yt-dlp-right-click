# Removes what Setup installed: the helper registration for Chrome and Edge,
# and the install folder (extension, helper, and tools Setup downloaded).
# Your downloaded videos and music are never touched, and tools Setup only
# pointed at (your own yt-dlp or ffmpeg elsewhere) are left alone.
param(
    [string]$InstallDir = "",
    [switch]$Yes
)

$ErrorActionPreference = "Stop"
$HostName = "com.ytdlp_right_click.native_host"
$StateKey = "HKCU:\Software\yt-dlp-right-click"

if (-not $InstallDir) {
    if (Test-Path $StateKey) { $InstallDir = [string](Get-Item $StateKey).GetValue("InstallDir") }
    if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "yt-dlp-right-click" }
}

Write-Host "This removes yt-dlp Right Click from: $InstallDir"
Write-Host "Your downloaded files are kept."
if (-not $Yes) {
    $answer = Read-Host "Continue? [y/N]"
    if ($answer -notmatch '^\s*y') {
        Write-Host "Nothing was changed."
        return
    }
}

foreach ($browserKey in "HKCU:\Software\Google\Chrome\NativeMessagingHosts", "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts") {
    $key = "$browserKey\$HostName"
    if (Test-Path -LiteralPath $key) {
        Remove-Item -LiteralPath $key -Force
    }
}
if (Test-Path $StateKey) { Remove-Item -Path $StateKey -Recurse -Force }
Write-Host "Helper program unregistered."

Get-Process ytdlp_native_host -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force -ErrorAction SilentlyContinue

# Only delete a folder that really is a yt-dlp Right Click install.
$looksInstalled = (Test-Path -LiteralPath (Join-Path $InstallDir "host\ytdlp_native_host.exe")) -and
    (Test-Path -LiteralPath (Join-Path $InstallDir "extension\manifest.json"))
if ($looksInstalled) {
    Remove-Item -LiteralPath $InstallDir -Recurse -Force
    Write-Host "Removed $InstallDir"
}
elseif (Test-Path -LiteralPath $InstallDir) {
    Write-Host "Left $InstallDir alone: it doesn't look like a yt-dlp Right Click install."
}

Write-Host ""
Write-Host "Last step: in Chrome, open chrome://extensions and click Remove on yt-dlp Right Click."
