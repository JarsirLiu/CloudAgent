param(
    [switch]$Purge
)

$ErrorActionPreference = "Stop"

$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local\bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }

foreach ($name in @("cloudagent.cmd")) {
    $path = Join-Path $BinDir $name
    if (Test-Path $path) {
        Remove-Item -LiteralPath $path -Force
        Write-Host "Removed $path"
    }
}

if (Test-Path $InstallRoot) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
    Write-Host "Removed $InstallRoot"
}

if ($Purge -and (Test-Path $DataDir)) {
    Remove-Item -LiteralPath $DataDir -Recurse -Force
    Write-Host "Removed $DataDir"
} else {
    Write-Host "Kept user data: $DataDir"
}
