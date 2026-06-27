param(
    [string]$Version = "latest",
    [switch]$Force,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$Repo = "JarsirLiu/CloudAgent"
$ScriptBaseUrl = if ($env:CLOUDAGENT_SCRIPT_BASE_URL) {
    $env:CLOUDAGENT_SCRIPT_BASE_URL
}
else {
    "https://raw.githubusercontent.com/$Repo/main/scripts"
}
$ScriptFallbackUrl = if ($env:CLOUDAGENT_SCRIPT_FALLBACK_URL) {
    $env:CLOUDAGENT_SCRIPT_FALLBACK_URL
}
else {
    "https://github.com/$Repo/releases/latest/download"
}
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) {
    $env:CLOUDAGENT_INSTALL_ROOT
}
elseif ($IsWindows -and $env:LOCALAPPDATA) {
    Join-Path $env:LOCALAPPDATA "CloudAgent"
}
else {
    Join-Path $HOME ".local/share/CloudAgent"
}
$InstallsDir = Join-Path $InstallRoot "installs"
$CurrentDir = Join-Path $InstallRoot "current"
$InstallMarker = ".cloudagent-install-complete"
$BinDir = if ($env:CLOUDAGENT_BIN_DIR) { $env:CLOUDAGENT_BIN_DIR } else { Join-Path $HOME ".local\bin" }
$DataDir = if ($env:CLOUDAGENT_DATA_DIR) { $env:CLOUDAGENT_DATA_DIR } else { Join-Path $HOME ".cloudagent" }
$script:LastDownloadStatusLength = 0
$script:CurlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue
$script:StageTotal = 6

$releaseTagRulesPath = if ($PSScriptRoot) { Join-Path $PSScriptRoot "release-tag-rules.ps1" } else { $null }
if ($releaseTagRulesPath -and (Test-Path $releaseTagRulesPath)) {
    . $releaseTagRulesPath
}
else {
    function Test-SemVerTag {
        param([Parameter(Mandatory = $true)][string]$Value)

        return $Value -match '^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$'
    }

    function Normalize-ReleaseTag {
        param([Parameter(Mandatory = $true)][string]$Version)

        $normalizedVersion = $Version.Trim()
        if (-not $normalizedVersion) {
            throw "invalid release version: $Version"
        }

        $releaseTag = if ($normalizedVersion.StartsWith("v")) { $normalizedVersion } else { "v$normalizedVersion" }
        if (-not (Test-SemVerTag $releaseTag)) {
            throw "invalid release version: $Version"
        }

        return $releaseTag
    }
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Resolve-RequestedVersion {
    param([string]$Value)

    $normalized = if ($null -eq $Value) { "" } else { $Value.Trim() }
    if (-not $normalized) {
        return "latest"
    }

    return $normalized
}

if ($SelfTest) {
    $validTags = @(
        "v0.1.0"
        "v1.2.3"
        "v1.2.3-beta.1"
        "v1.2.3+build.7"
        "v1.2.3-beta.1+build.7"
    )

    foreach ($validTag in $validTags) {
        Assert-True (Test-SemVerTag $validTag) "expected valid tag to pass: $validTag"
    }

    $invalidTags = @(
        "v"
        "v1"
        "v1.2"
        "1.2.3"
        "v01.2.3"
        "v1.02.3"
        "v1.2.03"
        "v1.2.3-"
        "v1.2.3+"
    )

    foreach ($invalidTag in $invalidTags) {
        Assert-True (-not (Test-SemVerTag $invalidTag)) "expected invalid tag to fail: $invalidTag"
    }

    Assert-True ((Normalize-ReleaseTag -Version "1.2.3") -eq "v1.2.3") "normalize-release-tag failed for 1.2.3"
    Assert-True ((Normalize-ReleaseTag -Version "v1.2.3-beta.1") -eq "v1.2.3-beta.1") "normalize-release-tag failed for v1.2.3-beta.1"
    Assert-True ((Resolve-RequestedVersion "") -eq "latest") "empty requested version should fall back to latest"
    Assert-True ((Resolve-RequestedVersion "  v1.2.3  ") -eq "v1.2.3") "requested version should be trimmed"

    Write-Host "install.ps1 self-test passed"
    return
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

    $directory = Split-Path -Parent $OutFile
    if ($directory) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    try {
        $invokeWebRequestParams = @{
            Uri = $Uri
            Headers = $Headers
            OutFile = $OutFile
            ErrorAction = "Stop"
        }
        if ($PSVersionTable.PSVersion.Major -lt 6) {
            $invokeWebRequestParams["UseBasicParsing"] = $true
        }

        Invoke-WebRequest @invokeWebRequestParams
        if (Test-Path $OutFile) {
            $length = (Get-Item $OutFile).Length
            Write-DownloadStatus -Label $Label -DownloadedBytes $length -TotalBytes $length
            Complete-DownloadStatus
        }
        return
    }
    catch {
        if ($script:CurlCommand) {
            if (Test-Path $OutFile) {
                Remove-Item -LiteralPath $OutFile -Force
            }

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
            try {
                & $script:CurlCommand.Source @curlArgs
                if ($LASTEXITCODE -ne 0) {
                    throw "curl.exe download failed for $Uri"
                }
                if (Test-Path $OutFile) {
                    $length = (Get-Item $OutFile).Length
                    Write-DownloadStatus -Label $Label -DownloadedBytes $length -TotalBytes $length
                    Complete-DownloadStatus
                }
                return
            }
            catch {
                if (Test-Path $OutFile) {
                    Remove-Item -LiteralPath $OutFile -Force
                }
                throw "Failed to download $Uri with Invoke-WebRequest and curl.exe. $($_.Exception.Message)"
            }
        }

        if (Test-Path $OutFile) {
            Remove-Item -LiteralPath $OutFile -Force
        }
        throw "Failed to download $Uri with Invoke-WebRequest. $($_.Exception.Message)"
    }
}

function Get-TargetAssetName {
    $arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { throw "Unsupported Windows architecture" }
    return "cloudagent-$script:ReleaseTag-windows-$arch.zip"
}

function Resolve-LatestReleaseTag {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ "User-Agent" = "cloudagent-installer" }
    if (-not $release.tag_name) {
        throw "Failed to resolve the latest release version."
    }

    $releaseTag = Normalize-ReleaseTag -Version ([string]$release.tag_name)
    if (-not (Test-SemVerTag $releaseTag)) {
        throw "Failed to resolve the latest release version."
    }

    return $releaseTag
}

function Get-ReleaseAssetMetadata {
    param(
        [string]$AssetName,
        [string]$ResolvedVersion
    )

    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$ResolvedVersion" -Headers @{ "User-Agent" = "cloudagent-installer" }
    $asset = $release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
    if ($null -eq $asset) {
        throw "Could not find release asset $AssetName for CloudAgent $ResolvedVersion."
    }

    $digestMatch = [regex]::Match([string]$asset.digest, "^sha256:([0-9a-fA-F]{64})$")
    if (-not $digestMatch.Success) {
        throw "Could not find SHA-256 digest for release asset $AssetName."
    }

    return [PSCustomObject]@{
        Url = [string]$asset.browser_download_url
        Sha256 = $digestMatch.Groups[1].Value.ToLowerInvariant()
    }
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

function Write-Launcher {
    $launcherPath = Join-Path $BinDir "cloudagent.cmd"
    $helperPath = Join-Path $BinDir "cloudagent-launch.ps1"

    @'
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args
)

$ErrorActionPreference = "Stop"
$ScriptBaseUrl = '__SCRIPT_BASE_URL__'
$ScriptFallbackUrl = '__SCRIPT_FALLBACK_URL__'
$CurrentDir = '__CURRENT_DIR__'
$script:CurlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue

function Get-RemainingArgs {
    param([string[]]$Arguments)

    if (-not $Arguments -or $Arguments.Count -le 1) {
        return @()
    }

    return @($Arguments[1..($Arguments.Count - 1)])
}

function Get-RemoteScriptBundle {
    param([Parameter(Mandatory = $true)][string]$FileName)

    switch ($FileName) {
        "upgrade.ps1" {
            return @(
                "upgrade.ps1"
                "install.ps1"
                "release-tag-rules.ps1"
            )
        }
        default {
            return @($FileName)
        }
    }
}

function Invoke-RemoteScript {
    param(
        [Parameter(Mandatory = $true)][string]$FileName,
        [string[]]$RemainingArgs = @()
    )

    $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("cloudagent-" + [guid]::NewGuid().ToString("N"))

    try {
        New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null

        foreach ($bundleFile in (Get-RemoteScriptBundle -FileName $FileName)) {
            $tempScript = Join-Path $tempRoot $bundleFile
            $downloaded = $false

            foreach ($baseUrl in @($ScriptBaseUrl, $ScriptFallbackUrl)) {
                if (-not $baseUrl) {
                    continue
                }

                $scriptUrl = ($baseUrl.TrimEnd('/') + '/' + $bundleFile)
                try {
                    if ($script:CurlCommand) {
                        & $script:CurlCommand.Source `
                            --fail `
                            --location `
                            --silent `
                            --show-error `
                            --output $tempScript `
                            $scriptUrl
                        if ($LASTEXITCODE -ne 0) {
                            throw "curl.exe download failed for $scriptUrl"
                        }
                    }
                    else {
                        Invoke-WebRequest -Uri $scriptUrl -Headers @{ "User-Agent" = "cloudagent-installer" } -OutFile $tempScript
                    }

                    $downloaded = $true
                    break
                }
                catch {
                    if (Test-Path $tempScript) {
                        Remove-Item -LiteralPath $tempScript -Force
                    }
                }
            }

            if (-not $downloaded) {
                throw "failed to download $bundleFile from configured script sources"
            }
        }

        & (Join-Path $tempRoot $FileName) @RemainingArgs
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
    }
    finally {
        if (Test-Path $tempRoot) {
            Remove-Item -LiteralPath $tempRoot -Recurse -Force
        }
    }
}

if (-not $Args -or $Args.Count -eq 0) {
    & (Join-Path $CurrentDir "cloudagent.exe")
    exit $LASTEXITCODE
}

switch ($Args[0]) {
    "upgrade" {
        Invoke-RemoteScript -FileName "upgrade.ps1" -RemainingArgs (Get-RemainingArgs -Arguments $Args)
        exit $LASTEXITCODE
    }
    "uninstall" {
        Invoke-RemoteScript -FileName "uninstall.ps1" -RemainingArgs (Get-RemainingArgs -Arguments $Args)
        exit $LASTEXITCODE
    }
    default {
        & (Join-Path $CurrentDir "cloudagent.exe") @Args
        exit $LASTEXITCODE
    }
}
'@ |
        ForEach-Object {
            $_.Replace('__SCRIPT_BASE_URL__', $ScriptBaseUrl).
               Replace('__SCRIPT_FALLBACK_URL__', $ScriptFallbackUrl).
               Replace('__CURRENT_DIR__', $CurrentDir)
        } | Set-Content -Encoding ASCII -Path $helperPath

    @"
@echo off
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0cloudagent-launch.ps1" %*
"@ | Set-Content -Encoding ASCII -Path $launcherPath
    Write-Host "Installed launcher: $launcherPath"
}

function Add-UserPathEntry {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) {
        $userPath = ""
    }
    $parts = $userPath.Split(';') | Where-Object { $_ }
    if ($parts | Where-Object { Test-PathEntryEquals $_ $BinDir } | Select-Object -First 1) {
        return $false
    }
    $newPath = ($parts + $BinDir) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    # Sync to current session for immediate availability
    $env:Path = "$BinDir;$env:Path"
    return $true
}

$headers = @{ "User-Agent" = "cloudagent-installer" }
$requestedVersion = Resolve-RequestedVersion $Version
Write-StageStart -Step 1 -Title "Resolving release metadata"
$script:ReleaseTag = if ($requestedVersion -eq "latest") { Resolve-LatestReleaseTag } else { Normalize-ReleaseTag $requestedVersion }
$releaseVersion = $script:ReleaseTag.TrimStart('v')
$assetName = Get-TargetAssetName
$assetMetadata = Get-ReleaseAssetMetadata -AssetName $assetName -ResolvedVersion $script:ReleaseTag
Write-StageDone -Detail "($script:ReleaseTag)"

$targetDir = Join-Path $InstallsDir $releaseVersion
$markerPath = Join-Path $targetDir $InstallMarker
if ((-not $Force) -and (Test-Path $markerPath) -and (Test-Path $CurrentDir) -and ((Get-Item $CurrentDir).Target -eq $targetDir)) {
    Write-Host "CloudAgent $releaseVersion is already installed"
    Write-Launcher
    if (Add-UserPathEntry) {
        Write-Host "Added launcher directory to PATH: $BinDir"
    }
    return
}

New-Item -ItemType Directory -Path $InstallsDir, $BinDir, $DataDir -Force | Out-Null
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "cloudagent-install-$PID"
New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
try {
    $zipPath = Join-Path $tempRoot $assetName
    Write-StageStart -Step 2 -Title "Downloading release asset"
    Invoke-DownloadFile `
        -Uri $assetMetadata.Url `
        -Headers $headers `
        -OutFile $zipPath `
        -Label "Downloading CloudAgent $releaseVersion"
    Write-StageDone -Detail ("({0})" -f (Format-ByteSize ((Get-Item $zipPath).Length)))

    Write-StageStart -Step 3 -Title "Verifying release asset"
    $actual = Get-Sha256Hash -Path $zipPath
    if ($assetMetadata.Sha256 -ne $actual) {
        throw "Checksum verification failed for $assetName"
    }
    Write-StageDone

    $unpackRoot = Join-Path $tempRoot "unpack"
    Write-StageStart -Step 4 -Title "Extracting package"
    Expand-Archive -LiteralPath $zipPath -DestinationPath $unpackRoot -Force
    $packageDir = Get-ChildItem -Path $unpackRoot -Directory | Select-Object -First 1
    if (-not $packageDir) {
        throw "Invalid archive layout: missing package directory"
    }
    Write-StageDone

    if (Test-Path $targetDir) {
        Write-Host "Replacing existing installation at $targetDir"
        Remove-Item -LiteralPath $targetDir -Recurse -Force
    }
    Write-StageStart -Step 5 -Title "Installing files"
    New-Item -ItemType Directory -Path $targetDir -Force | Out-Null
    Copy-Item -Path (Join-Path $packageDir.FullName "*") -Destination $targetDir -Recurse -Force
    Write-StageDone

    if (Test-Path $CurrentDir) {
        Write-Host "Updating current launcher target"
        Remove-Item -LiteralPath $CurrentDir -Recurse -Force
    }
    Write-StageStart -Step 6 -Title "Refreshing command launchers"
   New-Item -ItemType Junction -Path $CurrentDir -Target $targetDir | Out-Null
   Write-Launcher
    if (Add-UserPathEntry) {
        Write-Host "Added launcher directory to PATH: $BinDir"
    }
   Set-Content -Encoding ASCII -NoNewline -Path $markerPath -Value ""
   Write-StageDone

    Write-Host "CloudAgent $releaseVersion installed"
    Write-Host "Install root: $InstallRoot"
    Write-Host "Data dir: $DataDir"
    Write-Host "Launcher dir: $BinDir"
    Write-Host "Run: $BinDir\\cloudagent.cmd start"
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
