param(
    [switch]$Purge
)

$ErrorActionPreference = "Stop"

$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local\bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }

function Remove-UserPathEntry {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        return
    }

    $parts = $userPath.Split(';') | Where-Object { $_ }
    $filtered = $parts | Where-Object { $_ -ne $BinDir }
    if ($filtered.Count -eq $parts.Count) {
        return
    }

    $newPath = ($filtered -join ';')
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Host "Removed $BinDir from user PATH"
}

foreach ($name in @("cloudagent.cmd", "cloudagent-launch.ps1")) {
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

Remove-UserPathEntry
