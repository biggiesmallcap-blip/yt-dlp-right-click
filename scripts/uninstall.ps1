# Removes what Setup installed: the helper registration for Chrome and Edge,
# and the install folder (extension, helper, and downloaded tools).
# Your downloaded videos and music are never touched.
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "yt-dlp-right-click"),
    [switch]$Yes
)

$ErrorActionPreference = "Stop"
$HostName = "com.ytdlp_right_click.native_host"

if (-not $Yes) {
    $answer = Read-Host "Remove yt-dlp Right Click from this PC? Your downloads are kept. [y/N]"
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
Write-Host "Helper program unregistered."

Get-Process ytdlp_native_host -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force -ErrorAction SilentlyContinue

if (Test-Path -LiteralPath $InstallDir) {
    Remove-Item -LiteralPath $InstallDir -Recurse -Force
    Write-Host "Removed $InstallDir"
}

Write-Host ""
Write-Host "Last step: in Chrome, open chrome://extensions and click Remove on yt-dlp Right Click."
