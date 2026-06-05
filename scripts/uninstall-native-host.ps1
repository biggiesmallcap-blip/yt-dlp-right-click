param()

$ErrorActionPreference = "Stop"

$HostName = "com.ytdlp_right_click.native_host"
$RegistryPath = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$HostName"

if (Test-Path -LiteralPath $RegistryPath) {
    Remove-Item -LiteralPath $RegistryPath -Force
    Write-Host "Removed native host registry entry: $RegistryPath"
} else {
    Write-Host "Native host registry entry was not present: $RegistryPath"
}
