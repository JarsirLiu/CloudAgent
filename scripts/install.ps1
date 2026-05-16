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
$script:LastDownloadStatusLength = 0
$script:CurlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue

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
        $line = "$Label  $downloadedText / $totalText ($percent%)"
    } else {
        $line = "$Label  $downloadedText downloaded"
    }

    $padding = ""
    if ($script:LastDownloadStatusLength -gt $line.Length) {
        $padding = " " * ($script:LastDownloadStatusLength - $line.Length)
    }
    Write-Host -NoNewline ("`r" + $line + $padding)
    $script:LastDownloadStatusLength = $line.Length
}

function Complete-DownloadStatus {
    if ($script:LastDownloadStatusLength -gt 0) {
        Write-Host ""
        $script:LastDownloadStatusLength = 0
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
    $directory = Split-Path -Parent $OutFile
    if ($directory) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    if ($script:CurlCommand) {
        $curlArgs = @("--fail", "--location", "-o", $OutFile)
        foreach ($entry in $Headers.GetEnumerator()) {
            $curlArgs += @("-H", ("{0}: {1}" -f [string]$entry.Key, [string]$entry.Value))
        }
        if (-not [Console]::IsErrorRedirected) {
            $curlArgs += "--progress-bar"
        } else {
            $curlArgs += @("--silent", "--show-error")
        }
        $curlArgs += $Uri
        & $script:CurlCommand.Source @curlArgs
        if ($LASTEXITCODE -ne 0) {
            throw "curl.exe download failed for $Uri"
        }
        return
    }

    $request = [System.Net.WebRequest]::Create($Uri)
    $request.Method = "GET"
    foreach ($entry in $Headers.GetEnumerator()) {
        if ([string]$entry.Key -ieq "User-Agent") {
            $request.UserAgent = [string]$entry.Value
        }
        else {
            $request.Headers[[string]$entry.Key] = [string]$entry.Value
        }
    }

    $response = $null
    $responseStream = $null
    $fileStream = $null
    try {
        $response = $request.GetResponse()
        $totalBytes = $response.ContentLength
        if ($totalBytes -lt 0) {
            $totalBytes = $null
        }

        $responseStream = $response.GetResponseStream()
        $fileStream = [System.IO.File]::Open($OutFile, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        $buffer = New-Object byte[] (128KB)
        $downloadedBytes = 0L
        while (($read = $responseStream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            $fileStream.Write($buffer, 0, $read)
            $downloadedBytes += $read
            Write-DownloadStatus -Label $Label -DownloadedBytes $downloadedBytes -TotalBytes $totalBytes
        }
        Complete-DownloadStatus
    }
    finally {
        if ($fileStream) {
            $fileStream.Dispose()
        }
        if ($responseStream) {
            $responseStream.Dispose()
        }
        if ($response) {
            $response.Dispose()
        }
        Complete-DownloadStatus
    }
}

function Get-TargetAssetName {
    $arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { throw "Unsupported Windows architecture" }
    return "cloudagent-$script:ReleaseTag-windows-$arch.zip"
}

function Resolve-LatestReleaseTag {
    $latestUrl = "https://github.com/$Repo/releases/latest"

    if ($script:CurlCommand) {
        $effectiveUrl = & $script:CurlCommand.Source `
            --silent `
            --show-error `
            --location `
            --output NUL `
            --write-out "%{url_effective}" `
            $latestUrl
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to resolve latest release tag from $latestUrl"
        }
        return Split-Path -Leaf $effectiveUrl.Trim()
    }

    $currentUrl = $latestUrl
    for ($i = 0; $i -lt 5; $i++) {
        $request = [System.Net.HttpWebRequest]::Create($currentUrl)
        $request.Method = "HEAD"
        $request.AllowAutoRedirect = $false
        $request.UserAgent = "cloudagent-installer"
        try {
            $response = [System.Net.HttpWebResponse]$request.GetResponse()
            if ($response.StatusCode -ge 300 -and $response.StatusCode -lt 400 -and $response.Headers["Location"]) {
                $location = $response.Headers["Location"]
                if ($location.StartsWith("/")) {
                    $location = "https://github.com$location"
                }
                $currentUrl = $location
                continue
            }
            return Split-Path -Leaf $response.ResponseUri.AbsoluteUri
        }
        catch [System.Net.WebException] {
            $response = $_.Exception.Response
            if ($response -and $response.StatusCode -ge 300 -and $response.StatusCode -lt 400 -and $response.Headers["Location"]) {
                $location = $response.Headers["Location"]
                if ($location.StartsWith("/")) {
                    $location = "https://github.com$location"
                }
                $currentUrl = $location
                continue
            }
            throw
        }
    }

    throw "Failed to resolve latest release tag from $latestUrl"
}

function Get-Sha256Hash {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (Get-Command Get-FileHash -ErrorAction SilentlyContinue) {
        return (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLowerInvariant()
    }

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash($stream)
            return ([System.BitConverter]::ToString($hashBytes) -replace "-", "").ToLowerInvariant()
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
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
  powershell -NoProfile -ExecutionPolicy Bypass -Command "$ts=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); irm https://raw.githubusercontent.com/$Repo/main/scripts/upgrade.ps1?ts=$ts | iex"
  exit /b %ERRORLEVEL%
)
if /I "%CMD%"=="uninstall" (
  shift
  powershell -NoProfile -ExecutionPolicy Bypass -Command "$ts=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds(); irm https://raw.githubusercontent.com/$Repo/main/scripts/uninstall.ps1?ts=$ts | iex"
  exit /b %ERRORLEVEL%
)
"$CurrentDir\cloudagent.exe" %*
"@ | Set-Content -Encoding ASCII -Path $launcherPath
    Write-Host "Installed launcher: $launcherPath"
}

$headers = @{ "User-Agent" = "cloudagent-installer" }
Write-Host "Resolving release metadata"
$script:ReleaseTag = if ($Version -eq "latest") { Resolve-LatestReleaseTag } else { "v$Version" }
$releaseVersion = $script:ReleaseTag.TrimStart('v')
$assetName = Get-TargetAssetName
$assetUrl = "https://github.com/$Repo/releases/download/$script:ReleaseTag/$assetName"
$checksumsUrl = "https://github.com/$Repo/releases/download/$script:ReleaseTag/SHA256SUMS"

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
        -Uri $assetUrl `
        -Headers $headers `
        -OutFile $zipPath `
        -Label "Downloading CloudAgent $releaseVersion"

    $checksumPath = Join-Path $tempRoot "SHA256SUMS"
    Invoke-DownloadFile `
        -Uri $checksumsUrl `
        -Headers $headers `
        -OutFile $checksumPath `
        -Label "Downloading checksum manifest"
    Write-Host "Verifying package checksum"
    $expected = (Select-String -Path $checksumPath -Pattern ([regex]::Escape($assetName)) | Select-Object -First 1).Line.Split(' ')[0]
    $actual = Get-Sha256Hash -Path $zipPath
    if ($expected.ToLowerInvariant() -ne $actual) {
        throw "Checksum verification failed for $assetName"
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
