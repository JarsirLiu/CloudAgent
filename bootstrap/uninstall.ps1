param(
    [switch]$Purge
)

$ErrorActionPreference = "Stop"

$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local\bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }
$script:StageTotal = 3

function Get-CurrentVersion {
    $currentDir = Join-Path $InstallRoot "current"
    if (-not (Test-Path $currentDir)) {
        return $null
    }

    $item = Get-Item $currentDir -ErrorAction SilentlyContinue
    if (-not $item) {
        return $null
    }

    if ($item.Target) {
        return Split-Path -Leaf $item.Target
    }

    return Split-Path -Leaf $item.FullName
}

function Write-StageStart {
    param(
        [int]$Step,
        [string]$Title
    )

    Write-Host ("[{0}/{1}] {2}... " -f $Step, $script:StageTotal, $Title) -NoNewline
}

function Write-StageDone {
    param([string]$Detail = "")

    if ($Detail) {
        Write-Host ("done {0}" -f $Detail)
    }
    else {
        Write-Host "done"
    }
}

function Remove-UserPathEntry {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        return $false
    }

    $parts = $userPath.Split(';') | Where-Object { $_ }
    $filtered = $parts | Where-Object { $_ -ne $BinDir }
    if ($filtered.Count -eq $parts.Count) {
        return $false
    }

    $newPath = ($filtered -join ';')
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    return $true
}

Write-Host "🧹 Uninstalling CloudAgent"
$version = Get-CurrentVersion
if ($version) {
    Write-Host "CloudAgent $version"
}
Write-Host ""

Write-StageStart -Step 1 -Title "Removing launchers"
$launcherRemoved = $false
foreach ($name in @("cloudagent.cmd", "cloudagent-launch.ps1")) {
    $path = Join-Path $BinDir $name
    if (Test-Path $path) {
        Remove-Item -LiteralPath $path -Force
        $launcherRemoved = $true
    }
}
if (Remove-UserPathEntry) {
    $launcherRemoved = $true
}
if ($launcherRemoved) {
    Write-StageDone
}
else {
    Write-StageDone -Detail "(already removed)"
}

Write-StageStart -Step 2 -Title "Removing installation"
if (Test-Path $InstallRoot) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
    Write-StageDone
}
else {
    Write-StageDone -Detail "(already removed)"
}

$dataStageTitle = if ($Purge) { "Removing user data" } else { "Keeping user data" }
Write-StageStart -Step 3 -Title $dataStageTitle
if ($Purge -and (Test-Path $DataDir)) {
    Remove-Item -LiteralPath $DataDir -Recurse -Force
    Write-StageDone
    Write-Host "CloudAgent removed"
    Write-Host "User data removed: $DataDir"
}
else {
    Write-StageDone -Detail "(kept)"
    Write-Host "CloudAgent removed"
    Write-Host "User data kept: $DataDir"
}
