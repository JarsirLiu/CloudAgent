param(
    [switch]$Purge,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) {
    $env:CLOUDAGENT_INSTALL_ROOT
}
elseif ($IsWindows -and $env:LOCALAPPDATA) {
    Join-Path $env:LOCALAPPDATA "CloudAgent"
}
else {
    Join-Path $HOME ".local/share/CloudAgent"
}
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local/bin" }
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
    $filtered = $parts | Where-Object { $_ -ine $BinDir }
    if ($filtered.Count -eq $parts.Count) {
        return $false
    }

    $newPath = ($filtered -join ';')
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    return $true
}

function Disable-LauncherStub {
    param(
        [Parameter(Mandatory = $true)][string]$LauncherPath
    )

    if (-not (Test-Path $LauncherPath)) {
        return $false
    }

    @'
@echo off
rem CloudAgent has been removed. Reinstall to use this command again.
exit /b 0
'@ | Set-Content -Encoding ASCII -Path $LauncherPath
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

        if (-not (Disable-LauncherStub -LauncherPath (Join-Path $bin "cloudagent.cmd"))) {
            throw "expected cloudagent.cmd to be rewritten"
        }

        $stubContent = Get-Content -Raw -Path (Join-Path $bin "cloudagent.cmd")
        if ($stubContent -notmatch "CloudAgent has been removed") {
            throw "expected launcher stub content"
        }

        $binContent = Get-Content -Raw -Path (Join-Path $bin "cloudagent-launch.ps1")
        if ($binContent -notmatch "stub") {
            throw "expected helper stub content"
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

Write-Host "Uninstalling CloudAgent"
$version = Get-CurrentVersion
if ($version) {
    Write-Host "CloudAgent $version"
}
Write-Host ""

Write-StageStart -Step 1 -Title "Removing launchers"
$launcherRemoved = $false
if (Disable-LauncherStub -LauncherPath (Join-Path $BinDir "cloudagent.cmd")) {
    $launcherRemoved = $true
}
$helperPath = Join-Path $BinDir "cloudagent-launch.ps1"
if (Test-Path $helperPath) {
    Remove-Item -LiteralPath $helperPath -Force
    $launcherRemoved = $true
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
