param(
    [ValidatePattern('^[a-p]{32}$')]
    [string]$ExtensionId = "",

    [string]$HostExePath = "",

    [switch]$BuildRelease,

    [switch]$NoPrompt
)

$ErrorActionPreference = "Stop"

$HostName = "com.ytdlp_right_click.native_host"
$PublishedExtensionId = ""

$SourceProjectRoot = Split-Path -Parent $PSScriptRoot
$PackagedHostExePath = Join-Path $PSScriptRoot "ytdlp_native_host.exe"
$PackagedLayout = Test-Path -LiteralPath $PackagedHostExePath -PathType Leaf

if ($PackagedLayout) {
    $ProjectRoot = $PSScriptRoot
    $HostManifestDir = $PSScriptRoot
} else {
    $ProjectRoot = $SourceProjectRoot
    $HostManifestDir = Join-Path $ProjectRoot "native-host"
}

function Test-ExtensionId {
    param([string]$Value)
    return $Value -match '^[a-p]{32}$'
}

if ([string]::IsNullOrWhiteSpace($ExtensionId)) {
    if (Test-ExtensionId $env:YT_DLP_RIGHT_CLICK_EXTENSION_ID) {
        $ExtensionId = $env:YT_DLP_RIGHT_CLICK_EXTENSION_ID
    }
    elseif (Test-ExtensionId $PublishedExtensionId) {
        $ExtensionId = $PublishedExtensionId
    }
    elseif (-not $NoPrompt) {
        Write-Host "Chrome extension ID is needed once so Chrome can allow this native host."
        Write-Host "Open the extension settings page and copy the displayed Extension ID."
        $ExtensionId = Read-Host "Paste extension ID"
    }
}

if (-not (Test-ExtensionId $ExtensionId)) {
    throw "Extension ID must be 32 lowercase Chrome ID characters. For public releases, set `$PublishedExtensionId in this script before packaging."
}

if ($BuildRelease) {
    if ($PackagedLayout) {
        throw "-BuildRelease is only supported from the source tree, not from a packaged native host folder."
    }

    Push-Location (Join-Path $ProjectRoot "native-host")
    try {
        cargo build --release
    }
    finally {
        Pop-Location
    }
}

if ([string]::IsNullOrWhiteSpace($HostExePath)) {
    if ($PackagedLayout) {
        $HostExePath = $PackagedHostExePath
    } else {
        $HostExePath = Join-Path $ProjectRoot "native-host\target\release\ytdlp_native_host.exe"
    }
}

if (-not (Test-Path -LiteralPath $HostExePath -PathType Leaf)) {
    throw "Native host executable not found: $HostExePath. Build it first with: cargo build --release"
}

$ResolvedHost = (Resolve-Path -LiteralPath $HostExePath).Path
$ManifestPath = Join-Path $HostManifestDir "$HostName.json"

$Manifest = [ordered]@{
    name = $HostName
    description = "yt-dlp Right Click native messaging host"
    path = $ResolvedHost
    type = "stdio"
    allowed_origins = @("chrome-extension://$ExtensionId/")
}

$ManifestJson = $Manifest | ConvertTo-Json -Depth 4
[System.IO.File]::WriteAllText($ManifestPath, $ManifestJson, [System.Text.UTF8Encoding]::new($false))

$RegistryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"
New-Item -Path $RegistryPath -Force | Out-Null
Set-Item -Path $RegistryPath -Value $ManifestPath

Write-Host "Registered native host:"
Write-Host "  $HostName"
Write-Host "Manifest:"
Write-Host "  $ManifestPath"
Write-Host "Allowed extension:"
Write-Host "  chrome-extension://$ExtensionId/"
Write-Host ""
Write-Host "Next: reload the extension in Chrome, then click Test native host and settings."
