param(
    [string]$Version = "latest",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$Repo = "JarsirLiu/CloudAgent"
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$CurrentDir = Join-Path $InstallRoot "current"
$CurrentExe = Join-Path $CurrentDir "cloudagent.exe"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "cloudagent-upgrade-$PID"

function Test-NodeRunning {
    if (-not (Test-Path $CurrentExe)) {
        return $false
    }

    try {
        & $CurrentExe status *> $null
        return $LASTEXITCODE -eq 0
    }
    catch {
        return $false
    }
}

function Stop-NodeIfRunning {
    if (-not (Test-Path $CurrentExe)) {
        return $false
    }

    $wasRunning = Test-NodeRunning
    if (-not $wasRunning) {
        return $false
    }

    Write-Host "Stopping local node before upgrade"
    & $CurrentExe stop
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to stop the running local node before upgrade"
    }
    return $true
}

function Start-NodeAfterUpgrade {
    if (-not (Test-Path $CurrentExe)) {
        throw "Upgrade completed but cloudagent.exe is missing from $CurrentDir"
    }

    Write-Host "Starting local node after upgrade"
    & $CurrentExe start
    if ($LASTEXITCODE -ne 0) {
        throw "Upgrade installed successfully, but failed to restart the local node"
    }
}

function Invoke-InstallScript {
    if ($PSScriptRoot) {
        $localScript = Join-Path $PSScriptRoot "install.ps1"
        if (Test-Path $localScript) {
            & $localScript -Version $Version -Force:$Force
            return
        }
    }

    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    $installScript = Join-Path $tempRoot "install.ps1"
    Invoke-WebRequest `
        -Uri "https://raw.githubusercontent.com/$Repo/main/scripts/install.ps1" `
        -Headers @{ "User-Agent" = "cloudagent-upgrade" } `
        -OutFile $installScript
    & $installScript -Version $Version -Force:$Force
}

try {
    $restartNode = Stop-NodeIfRunning
    Invoke-InstallScript
    if ($restartNode) {
        Start-NodeAfterUpgrade
    }
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
