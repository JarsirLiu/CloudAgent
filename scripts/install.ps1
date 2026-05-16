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

function Format-ByteSize {
    param([double]$Bytes)

    if ($Bytes -ge 1GB) {
        return "{0:N1} GB" -f ($Bytes / 1GB)
    }
    if ($Bytes -ge 1MB) {
        return "{0:N1} MB" -f ($Bytes / 1MB)
    }
    if ($Bytes -ge 1KB) {
        return "{0:N1} KB" -f ($Bytes / 1KB)
    }
    return "{0:N0} B" -f $Bytes
}

function Write-DownloadStatus {
    param(
        [string]$Label,
        [long]$DownloadedBytes,
        [Nullable[long]]$TotalBytes
    )

    $downloadedText = Format-ByteSize $DownloadedBytes
    if ($TotalBytes.HasValue -and $TotalBytes.Value -gt 0) {
        $totalText = Format-ByteSize $TotalBytes.Value
        $percent = [math]::Min(100, [int](($DownloadedBytes * 100) / $TotalBytes.Value))
        Write-Progress -Activity $Label -Status "$downloadedText / $totalText" -PercentComplete $percent
    } else {
        Write-Progress -Activity $Label -Status "$downloadedText downloaded" -PercentComplete -1
    }
}

function Invoke-DownloadFile {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [Parameter(Mandatory = $true)][hashtable]$Headers,
        [Parameter(Mandatory = $true)][string]$Label
    )

    Write-Host $Label
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $Uri)
    foreach ($entry in $Headers.GetEnumerator()) {
        $request.Headers.TryAddWithoutValidation([string]$entry.Key, [string]$entry.Value) | Out-Null
    }

    $handler = [System.Net.Http.HttpClientHandler]::new()
    $client = [System.Net.Http.HttpClient]::new($handler)
    try {
        $response = $client.SendAsync(
            $request,
            [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead
        ).GetAwaiter().GetResult()
        $response.EnsureSuccessStatusCode()

        $totalBytes = $response.Content.Headers.ContentLength
        $stream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
        $directory = Split-Path -Parent $OutFile
        if ($directory) {
            New-Item -ItemType Directory -Path $directory -Force | Out-Null
        }
        $fileStream = [System.IO.File]::Open($OutFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        try {
            $buffer = New-Object byte[] (1024 * 128)
            $downloadedBytes = 0L
            while (($read = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $fileStream.Write($buffer, 0, $read)
                $downloadedBytes += $read
                Write-DownloadStatus -Label $Label -DownloadedBytes $downloadedBytes -TotalBytes $totalBytes
            }
            Write-Progress -Activity $Label -Completed
        }
        finally {
            $fileStream.Dispose()
            $stream.Dispose()
        }
    }
    finally {
        $request.Dispose()
        $client.Dispose()
        $handler.Dispose()
    }
}

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
Write-Host "Resolving release metadata"
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
    Invoke-DownloadFile `
        -Uri $asset.browser_download_url `
        -Headers $headers `
        -OutFile $zipPath `
        -Label "Downloading CloudAgent $releaseVersion"

    if ($checksums) {
        $checksumPath = Join-Path $tempRoot "SHA256SUMS"
        Invoke-DownloadFile `
            -Uri $checksums.browser_download_url `
            -Headers $headers `
            -OutFile $checksumPath `
            -Label "Downloading checksum manifest"
        Write-Host "Verifying package checksum"
        $expected = (Select-String -Path $checksumPath -Pattern ([regex]::Escape($assetName)) | Select-Object -First 1).Line.Split(' ')[0]
        $actual = (Get-FileHash -Algorithm SHA256 -Path $zipPath).Hash.ToLowerInvariant()
        if ($expected.ToLowerInvariant() -ne $actual) {
            throw "Checksum verification failed for $assetName"
        }
    }

    $unpackRoot = Join-Path $tempRoot "unpack"
    Write-Host "Extracting package"
    Expand-Archive -LiteralPath $zipPath -DestinationPath $unpackRoot -Force
    $packageDir = Get-ChildItem -Path $unpackRoot -Directory | Select-Object -First 1
    if (-not $packageDir) {
        throw "Invalid archive layout: missing package directory"
    }

    if (Test-Path $targetDir) {
        Write-Host "Replacing existing installation at $targetDir"
        Remove-Item -LiteralPath $targetDir -Recurse -Force
    }
    Write-Host "Installing files to $targetDir"
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Copy-Item -Path (Join-Path $packageDir.FullName "*") -Destination $targetDir -Recurse -Force

    if (Test-Path $CurrentDir) {
        Write-Host "Updating current launcher target"
        Remove-Item -LiteralPath $CurrentDir -Recurse -Force
    }
    New-Item -ItemType Junction -Path $CurrentDir -Target $targetDir | Out-Null

    Write-Host "Refreshing command launchers"
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
