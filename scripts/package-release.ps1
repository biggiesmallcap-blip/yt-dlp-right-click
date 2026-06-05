param(
    [string]$Version = "",
    [switch]$NoBuild
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$ManifestPath = Join-Path $ProjectRoot "extension\manifest.json"
$CargoTomlPath = Join-Path $ProjectRoot "native-host\Cargo.toml"
$GeneratedHostManifest = Join-Path $ProjectRoot "native-host\com.ytdlp_right_click.native_host.json"

function Assert-InProject {
    param([string]$Path)

    $projectFullPath = [System.IO.Path]::GetFullPath($ProjectRoot).TrimEnd('\', '/')
    $targetFullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $projectPrefix = $projectFullPath + [System.IO.Path]::DirectorySeparatorChar
    if ($targetFullPath -ne $projectFullPath -and -not $targetFullPath.StartsWith($projectPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to operate outside the project root: $targetFullPath"
    }
}

if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
    throw "Extension manifest not found: $ManifestPath"
}
if (-not (Test-Path -LiteralPath $CargoTomlPath -PathType Leaf)) {
    throw "Cargo.toml not found: $CargoTomlPath"
}

$Manifest = Get-Content -LiteralPath $ManifestPath -Raw | ConvertFrom-Json
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = [string]$Manifest.version
}
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') {
    throw "Version must be SemVer, for example 1.0.0 or 1.0.0-rc.1."
}
if ([string]$Manifest.version -ne $Version) {
    throw "extension\manifest.json version '$($Manifest.version)' does not match '$Version'."
}

$CargoToml = Get-Content -LiteralPath $CargoTomlPath -Raw
if ($CargoToml -notmatch "(?m)^version\s*=\s*`"$([regex]::Escape($Version))`"") {
    throw "native-host\Cargo.toml version does not match '$Version'."
}

if (Test-Path -LiteralPath $GeneratedHostManifest) {
    Write-Warning "Local generated native host manifest exists and will not be included: $GeneratedHostManifest"
}

if (-not $NoBuild) {
    Push-Location (Join-Path $ProjectRoot "native-host")
    try {
        cargo build --release
    }
    finally {
        Pop-Location
    }
}

$HostExePath = Join-Path $ProjectRoot "native-host\target\release\ytdlp_native_host.exe"
if (-not (Test-Path -LiteralPath $HostExePath -PathType Leaf)) {
    throw "Native host executable not found: $HostExePath"
}

$DistRoot = Join-Path $ProjectRoot "dist\$Version"
$StageRoot = Join-Path $DistRoot "_stage"
Assert-InProject $DistRoot
if (Test-Path -LiteralPath $DistRoot) {
    Remove-Item -LiteralPath $DistRoot -Recurse -Force
}

$ExtensionStage = Join-Path $StageRoot "extension"
$NativeStage = Join-Path $StageRoot "native-host"
New-Item -ItemType Directory -Path $ExtensionStage,$NativeStage -Force | Out-Null

Copy-Item -Path (Join-Path $ProjectRoot "extension\*") -Destination $ExtensionStage -Recurse
Copy-Item -LiteralPath $HostExePath -Destination (Join-Path $NativeStage "ytdlp_native_host.exe")
Copy-Item -LiteralPath (Join-Path $ProjectRoot "scripts\install-native-host.ps1") -Destination $NativeStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "scripts\install-native-host.cmd") -Destination $NativeStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "scripts\uninstall-native-host.ps1") -Destination $NativeStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "native-host\com.ytdlp_right_click.native_host.template.json") -Destination $NativeStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "README.md") -Destination $NativeStage
Copy-Item -LiteralPath (Join-Path $ProjectRoot "SECURITY.md") -Destination $NativeStage

$NativeDocs = Join-Path $NativeStage "docs"
New-Item -ItemType Directory -Path $NativeDocs -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $ProjectRoot "docs\INSTALL.md") -Destination $NativeDocs
Copy-Item -LiteralPath (Join-Path $ProjectRoot "docs\RELEASE.md") -Destination $NativeDocs

$ExtensionZip = Join-Path $DistRoot "yt-dlp-right-click-extension-v$Version.zip"
$NativeZip = Join-Path $DistRoot "yt-dlp-right-click-native-host-windows-v$Version.zip"
Compress-Archive -Path (Join-Path $ExtensionStage "*") -DestinationPath $ExtensionZip -Force
Compress-Archive -Path (Join-Path $NativeStage "*") -DestinationPath $NativeZip -Force

Remove-Item -LiteralPath $StageRoot -Recurse -Force

Write-Host "Release artifacts:"
Write-Host "  $ExtensionZip"
Write-Host "  $NativeZip"
