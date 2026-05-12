param(
    [string]$Version = "latest",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$Repo = "JarsirLiu/CloudAgent"
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$InstallsDir = Join-Path $InstallRoot "installs"
$CurrentDir = Join-Path $InstallRoot "current"
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local\bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }

function Get-ReleaseApiUrl {
    if ($Version -eq "latest") {
        return "https://api.github.com/repos/$Repo/releases/latest"
    }
    return "https://api.github.com/repos/$Repo/releases/tags/v$Version"
}

function Get-TargetAssetName {
    $arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { throw "Unsupported Windows architecture" }
    return "cloudagent-$script:ReleaseTag-windows-$arch.zip"
}

function Ensure-UserPath {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @()
    if ($userPath) {
        $parts = $userPath.Split(';') | Where-Object { $_ }
    }
    if ($parts -contains $BinDir) {
        return
    }
    $newPath = if ($userPath) { "$userPath;$BinDir" } else { $BinDir }
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = "$BinDir;$env:Path"
    Write-Host "Updated user PATH with $BinDir"
}

function Write-Launcher {
    $launcherPath = Join-Path $BinDir "cloudagent.cmd"
    @"
@echo off
set CMD=%1
if /I "%CMD%"=="upgrade" (
  shift
  powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/$Repo/main/scripts/upgrade.ps1 | iex"
  exit /b %ERRORLEVEL%
)
if /I "%CMD%"=="uninstall" (
  shift
  powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/$Repo/main/scripts/uninstall.ps1 | iex"
  exit /b %ERRORLEVEL%
)
"$CurrentDir\cloudagent.exe" %*
"@ | Set-Content -Encoding ASCII -Path $launcherPath
    Write-Host "Installed launcher: $launcherPath"
}

$headers = @{ "User-Agent" = "cloudagent-installer" }
$release = Invoke-RestMethod -Uri (Get-ReleaseApiUrl) -Headers $headers
$script:ReleaseTag = $release.tag_name
if (-not $script:ReleaseTag) {
    throw "Failed to resolve release tag"
}
$releaseVersion = $script:ReleaseTag.TrimStart('v')
$assetName = Get-TargetAssetName
$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
$checksums = $release.assets | Where-Object { $_.name -eq "SHA256SUMS" } | Select-Object -First 1
if (-not $asset) {
    throw "Could not find asset $assetName in release $script:ReleaseTag"
}

$targetDir = Join-Path $InstallsDir $releaseVersion
if ((-not $Force) -and (Test-Path $targetDir) -and (Test-Path $CurrentDir) -and ((Get-Item $CurrentDir).Target -eq $targetDir)) {
    Write-Host "CloudAgent $releaseVersion is already installed"
    exit 0
}

New-Item -ItemType Directory -Path $InstallsDir, $BinDir, $DataDir -Force | Out-Null
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "cloudagent-install-$PID"
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $zipPath = Join-Path $tempRoot $assetName
    Write-Host "Downloading $($asset.browser_download_url)"
    Invoke-WebRequest -Uri $asset.browser_download_url -Headers $headers -OutFile $zipPath

    if ($checksums) {
        $checksumPath = Join-Path $tempRoot "SHA256SUMS"
        Invoke-WebRequest -Uri $checksums.browser_download_url -Headers $headers -OutFile $checksumPath
        $expected = (Select-String -Path $checksumPath -Pattern ([regex]::Escape($assetName)) | Select-Object -First 1).Line.Split(' ')[0]
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zipPath).Hash.ToLowerInvariant()
        if ($expected.ToLowerInvariant() -ne $actual) {
            throw "Checksum verification failed for $assetName"
        }
    }

    $unpackRoot = Join-Path $tempRoot "unpack"
    Expand-Archive -LiteralPath $zipPath -DestinationPath $unpackRoot -Force
    $packageDir = Get-ChildItem -Path $unpackRoot -Directory | Select-Object -First 1
    if (-not $packageDir) {
        throw "Invalid archive layout: missing package directory"
    }

    if (Test-Path $targetDir) {
        Remove-Item -LiteralPath $targetDir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Copy-Item -Path (Join-Path $packageDir.FullName "*") -Destination $targetDir -Recurse -Force

    if (Test-Path $CurrentDir) {
        Remove-Item -LiteralPath $CurrentDir -Recurse -Force
    }
    New-Item -ItemType Junction -Path $CurrentDir -Target $targetDir | Out-Null

    Write-Launcher
    Ensure-UserPath

    Write-Host "Installed CloudAgent $releaseVersion"
    Write-Host "Install root: $InstallRoot"
    Write-Host "Data dir: $DataDir"
    Write-Host "Run: cloudagent start"
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
