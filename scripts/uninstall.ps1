param(
    [switch]$Purge,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$DefaultInstallRoot = if ($IsWindows -and $env:LOCALAPPDATA) {
    Join-Path $env:LOCALAPPDATA "CloudAgent"
}
else {
    Join-Path $HOME ".local/share/cloudagent"
}
$LegacyInstallRoot = Join-Path $HOME ".local/lib/cloudagent"
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) {
    $env:CLOUDAGENT_INSTALL_ROOT
}
else {
    $DefaultInstallRoot
}
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local/bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }
$script:StageTotal = 4

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

function Resolve-InstallRoot {
    if ($env:CLOUDAGENT_INSTALL_ROOT) {
        return $env:CLOUDAGENT_INSTALL_ROOT
    }

    if ($PSScriptRoot) {
        $scriptDir = $PSScriptRoot
        $supportParent = Split-Path -Parent $scriptDir
        if ($supportParent) {
            if ((Split-Path -Leaf $supportParent) -eq "current") {
                return (Split-Path -Parent $supportParent)
            }

            $releaseDir = Split-Path -Parent $supportParent
            if ($releaseDir -and ((Split-Path -Leaf $releaseDir) -eq "releases")) {
                return (Split-Path -Parent $releaseDir)
            }
        }
    }

    if ((-not $IsWindows) -and (Test-Path (Join-Path $LegacyInstallRoot "current"))) {
        return $LegacyInstallRoot
    }

    return $DefaultInstallRoot
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

function Normalize-PathEntry {
    param([Parameter(Mandatory = $true)][string]$Path)

    return $Path.Trim().TrimEnd('\')
}

function Test-PathEntryEquals {
    param(
        [Parameter(Mandatory = $true)][string]$Left,
        [Parameter(Mandatory = $true)][string]$Right
    )

    return (Normalize-PathEntry $Left).Equals((Normalize-PathEntry $Right), [System.StringComparison]::OrdinalIgnoreCase)
}

function Remove-UserPathEntry {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        return $false
    }

    $parts = $userPath.Split(';') | Where-Object { $_ }
    $filtered = $parts | Where-Object { -not (Test-PathEntryEquals $_ $BinDir) }
    if ($filtered.Count -eq $parts.Count) {
        return $false
    }

    $newPath = ($filtered -join ';')
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = (($env:Path -split ';') | Where-Object { $_ -and (-not (Test-PathEntryEquals $_ $BinDir)) }) -join ';'
    return $true
}

function Remove-LauncherFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path
    )

    if (-not (Test-Path $Path)) {
        return $false
    }

    Remove-Item -LiteralPath $Path -Force
    return $true
}

function Get-ManagedProcessIds {
    if (-not (Test-Path $InstallRoot)) {
        return @()
    }

    @(Get-CimInstance Win32_Process | Where-Object {
        $_.ExecutablePath -and
        $_.ExecutablePath.StartsWith($InstallRoot, [System.StringComparison]::OrdinalIgnoreCase) -and
        ($_.Name -eq "node.exe" -or $_.Name -eq "agentd.exe")
    } | Select-Object -ExpandProperty ProcessId)
}

function Test-NodeRunning {
    if (-not (Test-Path $CurrentNode)) {
        return $false
    }

    @(Get-CimInstance Win32_Process | Where-Object {
        $_.ExecutablePath -and
        $_.ExecutablePath.Equals($CurrentNode, [System.StringComparison]::OrdinalIgnoreCase)
    }).Count -gt 0
}

function Stop-ManagedProcessesIfRunning {
    if (-not (Test-NodeRunning)) {
        Write-StageStart -Step 1 -Title "Checking local node"
        Write-StageDone -Detail "(not running)"
        return $false
    }

    Write-StageStart -Step 1 -Title "Stopping local node"
    $processIds = Get-ManagedProcessIds
    if ($processIds.Count -gt 0) {
        Stop-Process -Id $processIds -Force
    }
    Write-StageDone -Detail "(stopped)"
    return $true
}

function Invoke-SelfTest {
    $tmpRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("cloudagent-uninstall-test-" + $PID)
    if (Test-Path $tmpRoot) {
        Remove-Item -LiteralPath $tmpRoot -Recurse -Force
    }

    $oldBinDir = $BinDir
    $oldInstallRoot = $InstallRoot
    $oldDataDir = $DataDir
    $oldUserPath = [Environment]::GetEnvironmentVariable("Path", "User")

    try {
        $tempHome = Join-Path $tmpRoot "home"
        $bin = Join-Path $tmpRoot "bin"
        $installRoot = Join-Path $tmpRoot "install"
        $dataDir = Join-Path $tmpRoot "data"

        New-Item -ItemType Directory -Path $tempHome, $bin, $installRoot, $dataDir, (Join-Path $tempHome ".config\fish") -Force | Out-Null

        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".profile") -Value @"
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
"@
        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".bashrc") -Value @"
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
"@
        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".zshrc") -Value @"
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
"@
        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".zprofile") -Value @"
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
"@
        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".bash_profile") -Value @"
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
"@
        Set-Content -Encoding ASCII -Path (Join-Path $tempHome ".config\fish\config.fish") -Value @"
# CloudAgent
fish_add_path "$HOME/.local/bin"
"@

        Set-Content -Encoding ASCII -Path (Join-Path $bin "cloudagent.cmd") -Value "@echo off`r`nexit /b 0`r`n"
        Set-Content -Encoding ASCII -Path (Join-Path $bin "cloudagent-launch.ps1") -Value "Write-Host stub"
        Set-Content -Encoding ASCII -Path (Join-Path $bin "cli") -Value "stub"
        Set-Content -Encoding ASCII -Path (Join-Path $bin "node") -Value "stub"
        Set-Content -Encoding ASCII -Path (Join-Path $bin "agentd") -Value "stub"

        $script:BinDir = $bin
        $script:InstallRoot = $installRoot
        $script:DataDir = $dataDir

        if (-not (Remove-LauncherFile -Path (Join-Path $bin "cloudagent.cmd"))) {
            throw "expected cloudagent.cmd to be removed"
        }

        if (Test-Path (Join-Path $bin "cloudagent.cmd")) {
            throw "expected cloudagent.cmd to be deleted"
        }

        if (-not (Remove-LauncherFile -Path (Join-Path $bin "cloudagent-launch.ps1"))) {
            throw "expected cloudagent-launch.ps1 to be removed"
        }

        if (Test-Path (Join-Path $bin "cloudagent-launch.ps1")) {
            throw "expected cloudagent-launch.ps1 to be deleted"
        }

        if ($IsWindows) {
            [Environment]::SetEnvironmentVariable("Path", "$bin;$oldUserPath", "User")
            if (-not (Remove-UserPathEntry)) {
                throw "expected user PATH cleanup to run"
            }
        }
        else {
            Remove-UserPathEntry | Out-Null
        }

        Write-Host "uninstall.ps1 self-test passed"
    }
    finally {
        $script:BinDir = $oldBinDir
        $script:InstallRoot = $oldInstallRoot
        $script:DataDir = $oldDataDir
        if ($IsWindows) {
            [Environment]::SetEnvironmentVariable("Path", $oldUserPath, "User")
        }
        if (Test-Path $tmpRoot) {
            Remove-Item -LiteralPath $tmpRoot -Recurse -Force
        }
    }
}

if ($SelfTest) {
    Invoke-SelfTest
    exit 0
}

$InstallRoot = Resolve-InstallRoot
$CurrentDir = Join-Path $InstallRoot "current"
$CurrentNode = Join-Path $CurrentDir "node.exe"

Write-Host "Uninstalling CloudAgent"
$version = Get-CurrentVersion
if ($version) {
    Write-Host "CloudAgent $version"
}
Write-Host ""

Stop-ManagedProcessesIfRunning | Out-Null

Write-StageStart -Step 2 -Title "Removing launchers"
$launcherRemoved = $false
if (Remove-LauncherFile -Path (Join-Path $BinDir "cloudagent.cmd")) {
    $launcherRemoved = $true
}
$helperPath = Join-Path $BinDir "cloudagent-launch.ps1"
if (Remove-LauncherFile -Path $helperPath) {
    $launcherRemoved = $true
}
foreach ($name in @("cli", "node", "agentd")) {
    if (Remove-LauncherFile -Path (Join-Path $BinDir $name)) {
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

Write-StageStart -Step 3 -Title "Removing installation"
if (Test-Path $InstallRoot) {
    Remove-Item -LiteralPath $InstallRoot -Recurse -Force
    Write-StageDone
}
else {
    Write-StageDone -Detail "(already removed)"
}

$dataStageTitle = if ($Purge) { "Removing user data" } else { "Keeping user data" }
Write-StageStart -Step 4 -Title $dataStageTitle
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
