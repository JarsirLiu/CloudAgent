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
$MetadataBaseUrl = if ($env:CLOUDAGENT_METADATA_BASE_URL) {
    $env:CLOUDAGENT_METADATA_BASE_URL
}
else {
    "https://github.com/$Repo/releases/latest/download"
}
$MetadataFallbackUrl = if ($env:CLOUDAGENT_METADATA_FALLBACK_URL) {
    $env:CLOUDAGENT_METADATA_FALLBACK_URL
}
else {
    "https://raw.githubusercontent.com/$Repo/main/scripts"
}
$ReleaseChannel = if ($env:CLOUDAGENT_RELEASE_CHANNEL) {
    $env:CLOUDAGENT_RELEASE_CHANNEL
}
else {
    "stable"
}
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
$ReleasesDir = Join-Path $InstallRoot "releases"
$CurrentDir = Join-Path $InstallRoot "current"
$InstallMarker = ".cloudagent-install-complete"
$SupportDirName = "support"
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

function Get-LocalScriptDirectory {
    if ($PSScriptRoot) {
        return $PSScriptRoot
    }

    $invocationPath = $MyInvocation.MyCommand.Path
    if (-not $invocationPath) {
        return $null
    }

    return (Split-Path -Parent $invocationPath)
}

function Resolve-InstalledScriptRoot {
    $scriptDir = Get-LocalScriptDirectory
    if (-not $scriptDir) {
        return $null
    }

    $supportParent = Split-Path -Parent $scriptDir
    if (-not $supportParent) {
        return $null
    }

    if ((Split-Path -Leaf $supportParent) -eq "current") {
        return (Split-Path -Parent $supportParent)
    }

    $releaseDir = Split-Path -Parent $supportParent
    if ($releaseDir -and ((Split-Path -Leaf $releaseDir) -eq "releases")) {
        return (Split-Path -Parent $releaseDir)
    }

    return $null
}

function Test-InstallRootPresent {
    param([Parameter(Mandatory = $true)][string]$Root)

    return (Test-Path (Join-Path $Root "current")) -or
        (Test-Path (Join-Path $Root "releases")) -or
        (Test-Path (Join-Path $Root "installs"))
}

function Resolve-InstallRoot {
    if ($env:CLOUDAGENT_INSTALL_ROOT) {
        return $env:CLOUDAGENT_INSTALL_ROOT
    }

    $scriptInstallRoot = Resolve-InstalledScriptRoot
    if ($scriptInstallRoot) {
        return $scriptInstallRoot
    }

    if (Test-InstallRootPresent -Root $DefaultInstallRoot) {
        return $DefaultInstallRoot
    }

    if ((-not $IsWindows) -and (Test-InstallRootPresent -Root $LegacyInstallRoot)) {
        return $LegacyInstallRoot
    }

    return $DefaultInstallRoot
}

$InstallRoot = Resolve-InstallRoot
$ReleasesDir = Join-Path $InstallRoot "releases"
$CurrentDir = Join-Path $InstallRoot "current"

function Get-SupportScriptNames {
    return @("install.ps1", "upgrade.ps1", "uninstall.ps1", "release-tag-rules.ps1")
}

function Get-ScriptDownloadBaseUrls {
    param([string]$ResolvedVersion)

    $baseUrls = @()
    if ($ResolvedVersion) {
        $baseUrls += "https://github.com/$Repo/releases/download/$ResolvedVersion"
    }
    $baseUrls += @($ScriptBaseUrl, $ScriptFallbackUrl)

    @($baseUrls | Where-Object { $_ } | Select-Object -Unique)
}

if ($SelfTest) {
    $originalDefaultInstallRoot = $DefaultInstallRoot
    $originalLegacyInstallRoot = $LegacyInstallRoot
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
    Assert-True ((Resolve-InstallRoot) -eq $DefaultInstallRoot) "resolve-install-root should use the default install root when no override exists"

    $resolveInstallRootTestDir = Join-Path ([System.IO.Path]::GetTempPath()) ("cloudagent-install-root-test-" + $PID)
    if (Test-Path $resolveInstallRootTestDir) {
        Remove-Item -LiteralPath $resolveInstallRootTestDir -Recurse -Force
    }

    try {
        $DefaultInstallRoot = Join-Path $resolveInstallRootTestDir "default"
        $LegacyInstallRoot = Join-Path $resolveInstallRootTestDir "legacy"
        New-Item -ItemType Directory -Path (Join-Path $DefaultInstallRoot "releases"), (Join-Path $LegacyInstallRoot "releases") -Force | Out-Null

        Assert-True ((Resolve-InstallRoot) -eq $DefaultInstallRoot) "resolve-install-root should prefer the default install root when both roots exist"

        Remove-Item -LiteralPath $DefaultInstallRoot -Recurse -Force
        Assert-True ((Resolve-InstallRoot) -eq $LegacyInstallRoot) "resolve-install-root should fall back to the legacy install root when the default root is absent"
    }
    finally {
        $DefaultInstallRoot = $originalDefaultInstallRoot
        $LegacyInstallRoot = $originalLegacyInstallRoot
        if (Test-Path $resolveInstallRootTestDir) {
            Remove-Item -LiteralPath $resolveInstallRootTestDir -Recurse -Force
        }
    }

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

function Get-TargetAssetKey {
    $arch = if ([Environment]::Is64BitOperatingSystem) { "x64" } else { throw "Unsupported Windows architecture" }
    return "windows-$arch"
}

function Get-LatestMetadata {
    if ($script:LatestMetadataLoaded) {
        return $script:LatestMetadata
    }

    foreach ($baseUrl in @($MetadataBaseUrl, $MetadataFallbackUrl)) {
        if (-not $baseUrl) {
            continue
        }

        foreach ($metadataName in @("$ReleaseChannel.json", "latest.json")) {
            try {
                $metadata = Invoke-RestMethod -Uri ($baseUrl.TrimEnd('/') + "/$metadataName") -Headers @{ "User-Agent" = "cloudagent-installer" }
                $script:LatestMetadataLoaded = $true
                $script:LatestMetadata = $metadata
                return $metadata
            }
            catch {
            }
        }
    }

    $script:LatestMetadataLoaded = $true
    $script:LatestMetadata = $null
    return $null
}

function Get-MetadataReleaseTag {
    param([Parameter(Mandatory = $true)]$Metadata)

    if ($Metadata.tag) {
        return Normalize-ReleaseTag -Version ([string]$Metadata.tag)
    }

    if ($Metadata.version) {
        return Normalize-ReleaseTag -Version ([string]$Metadata.version)
    }

    if ($Metadata.stable) {
        return Normalize-ReleaseTag -Version ([string]$Metadata.stable)
    }

    return $null
}

function Resolve-LatestReleaseTag {
    $metadata = Get-LatestMetadata
    if ($metadata) {
        $releaseTag = Get-MetadataReleaseTag -Metadata $metadata
        if ($releaseTag -and (Test-SemVerTag $releaseTag)) {
            return $releaseTag
        }
    }

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

    $assetKey = Get-TargetAssetKey
    if ($assetKey) {
        $metadata = Get-LatestMetadata
        if ($metadata) {
            $metadataReleaseTag = Get-MetadataReleaseTag -Metadata $metadata
            if ($metadataReleaseTag -and ($metadataReleaseTag -eq $ResolvedVersion)) {
                $asset = $metadata.assets.$assetKey
                if ($asset.url -and $asset.sha256) {
                    return [PSCustomObject]@{
                        Url = [string]$asset.url
                        Sha256 = ([string]$asset.sha256).ToLowerInvariant()
                    }
                }
            }
        }
    }

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

function Get-CurrentTargetPath {
    if (-not (Test-Path $CurrentDir)) {
        return $null
    }

    $item = Get-Item $CurrentDir -ErrorAction SilentlyContinue
    if (-not $item) {
        return $null
    }

    if ($item.Target) {
        return [string]$item.Target
    }

    return $item.FullName
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
$CurrentDir = '__CURRENT_DIR__'
$SupportDirName = '__SUPPORT_DIR_NAME__'

function Get-RemainingArgs {
    param([string[]]$Arguments)

    if (-not $Arguments -or $Arguments.Count -le 1) {
        return @()
    }

    $remaining = @($Arguments[1..($Arguments.Count - 1)])
    return @($remaining | Where-Object { $null -ne $_ -and $_.Length -gt 0 })
}

function Invoke-SupportScript {
    param(
        [Parameter(Mandatory = $true)][string]$FileName,
        [string[]]$RemainingArgs = @()
    )

    $localScript = Join-Path (Join-Path $CurrentDir $SupportDirName) $FileName
    if (Test-Path $localScript) {
        & $localScript @RemainingArgs
        if ($LASTEXITCODE -ne 0) {
            exit $LASTEXITCODE
        }
        return
    }

    Write-Error "Missing local support script: $localScript. Re-run the bootstrap installer to repair this installation."
    exit 1
}

if (-not $Args -or $Args.Count -eq 0) {
    & (Join-Path $CurrentDir "cloudagent.exe")
    exit $LASTEXITCODE
}

switch ($Args[0]) {
    "upgrade" {
        Invoke-SupportScript -FileName "upgrade.ps1" -RemainingArgs (Get-RemainingArgs -Arguments $Args)
        exit $LASTEXITCODE
    }
    "uninstall" {
        Invoke-SupportScript -FileName "uninstall.ps1" -RemainingArgs (Get-RemainingArgs -Arguments $Args)
        exit $LASTEXITCODE
    }
    default {
        & (Join-Path $CurrentDir "cloudagent.exe") @Args
        exit $LASTEXITCODE
    }
}
'@ |
        ForEach-Object {
            $_.Replace('__CURRENT_DIR__', $CurrentDir).
               Replace('__SUPPORT_DIR_NAME__', $SupportDirName)
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

$targetDir = Join-Path $ReleasesDir $releaseVersion
$markerPath = Join-Path $targetDir $InstallMarker
$currentTarget = Get-CurrentTargetPath
if ((-not $Force) -and (Test-Path $markerPath) -and $currentTarget -and (Test-PathEntryEquals $currentTarget $targetDir)) {
    Write-Host "CloudAgent $releaseVersion is already installed"
    Write-Launcher
    if (Add-UserPathEntry) {
        Write-Host "Added launcher directory to PATH: $BinDir"
    }
    return
}

function Copy-SupportScripts {
    param(
        [Parameter(Mandatory = $true)][string]$TargetDir,
        [Parameter(Mandatory = $true)][string]$ResolvedVersion,
        [Parameter(Mandatory = $true)][hashtable]$Headers
    )

    $supportDir = Join-Path $TargetDir $SupportDirName
    New-Item -ItemType Directory -Path $supportDir -Force | Out-Null

    $supportScriptNames = Get-SupportScriptNames
    $sourceDir = Get-LocalScriptDirectory
    if ($sourceDir) {
        $allLocalScriptsPresent = $true
        foreach ($fileName in $supportScriptNames) {
            if (-not (Test-Path (Join-Path $sourceDir $fileName))) {
                $allLocalScriptsPresent = $false
                break
            }
        }

        if ($allLocalScriptsPresent) {
            foreach ($fileName in $supportScriptNames) {
                Copy-Item -LiteralPath (Join-Path $sourceDir $fileName) -Destination (Join-Path $supportDir $fileName) -Force
            }
            return
        }
    }

    foreach ($fileName in $supportScriptNames) {
        $destinationPath = Join-Path $supportDir $fileName
        $downloaded = $false

        foreach ($baseUrl in (Get-ScriptDownloadBaseUrls -ResolvedVersion $ResolvedVersion)) {
            try {
                Invoke-DownloadFile `
                    -Uri ($baseUrl.TrimEnd('/') + '/' + $fileName) `
                    -Headers $Headers `
                    -OutFile $destinationPath `
                    -Label "Downloading support script $fileName"
                $downloaded = $true
                break
            }
            catch {
                if (Test-Path $destinationPath) {
                    Remove-Item -LiteralPath $destinationPath -Force
                }
            }
        }

        if (-not $downloaded) {
            throw "Failed to stage support script $fileName for CloudAgent $ResolvedVersion."
        }
    }
}

New-Item -ItemType Directory -Path $ReleasesDir, $BinDir, $DataDir -Force | Out-Null
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
    foreach ($requiredFile in @("cloudagent.exe", "cli.exe", "node.exe", "agentd.exe")) {
        if (-not (Test-Path (Join-Path $packageDir.FullName $requiredFile))) {
            throw "Invalid archive layout: missing $requiredFile"
        }
    }
    Write-StageDone

    $stagingRoot = Join-Path $InstallRoot ".staging"
    $stagedTargetDir = Join-Path $stagingRoot ("release-" + $releaseVersion + "-" + $PID)
    if (Test-Path $stagedTargetDir) {
        Remove-Item -LiteralPath $stagedTargetDir -Recurse -Force
    }
    Write-StageStart -Step 5 -Title "Installing files"
    New-Item -ItemType Directory -Path $stagingRoot, $stagedTargetDir -Force | Out-Null
    Copy-Item -Path (Join-Path $packageDir.FullName "*") -Destination $stagedTargetDir -Recurse -Force
    Copy-SupportScripts -TargetDir $stagedTargetDir -ResolvedVersion $script:ReleaseTag -Headers $headers
    $stagedMarkerPath = Join-Path $stagedTargetDir $InstallMarker
    Set-Content -Encoding ASCII -NoNewline -Path $stagedMarkerPath -Value ""
    Write-StageDone

    $currentTarget = Get-CurrentTargetPath
    if ($currentTarget -and (Test-PathEntryEquals $currentTarget $targetDir)) {
        throw "Refusing to replace the active version in place. Install a different version or uninstall first."
    }

    $backupTargetDir = $null
    if (Test-Path $targetDir) {
        $backupTargetDir = Join-Path $stagingRoot ("backup-" + $releaseVersion + "-" + $PID)
        if (Test-Path $backupTargetDir) {
            Remove-Item -LiteralPath $backupTargetDir -Recurse -Force
        }
        Write-Host "Replacing existing installation at $targetDir"
        Move-Item -LiteralPath $targetDir -Destination $backupTargetDir
    }

    try {
        Move-Item -LiteralPath $stagedTargetDir -Destination $targetDir
    }
    catch {
        if ($backupTargetDir -and (-not (Test-Path $targetDir)) -and (Test-Path $backupTargetDir)) {
            Move-Item -LiteralPath $backupTargetDir -Destination $targetDir -ErrorAction SilentlyContinue
        }
        throw "Failed to move the staged release into place. $($_.Exception.Message)"
    }

    if ($backupTargetDir -and (Test-Path $backupTargetDir)) {
        Remove-Item -LiteralPath $backupTargetDir -Recurse -Force
    }

    Write-StageStart -Step 6 -Title "Refreshing command launchers"
    $previousCurrentDir = $null
    $currentBackupDir = $null
    $temporaryCurrentDir = Join-Path $stagingRoot (".current-" + $PID)
    if (Test-Path $temporaryCurrentDir) {
        Remove-Item -LiteralPath $temporaryCurrentDir -Recurse -Force
    }
    New-Item -ItemType Junction -Path $temporaryCurrentDir -Target $targetDir | Out-Null
    if (Test-Path $CurrentDir) {
        Write-Host "Updating current launcher target"
        $currentBackupDir = Join-Path $stagingRoot (".current-backup-" + $PID)
        if (Test-Path $currentBackupDir) {
            Remove-Item -LiteralPath $currentBackupDir -Recurse -Force
        }
        Move-Item -LiteralPath $CurrentDir -Destination $currentBackupDir
        $previousCurrentDir = $currentBackupDir
    }
    try {
        Move-Item -LiteralPath $temporaryCurrentDir -Destination $CurrentDir
    }
    catch {
        if ($previousCurrentDir -and (Test-Path $previousCurrentDir) -and (-not (Test-Path $CurrentDir))) {
            Move-Item -LiteralPath $previousCurrentDir -Destination $CurrentDir -ErrorAction SilentlyContinue
        }
        if (Test-Path $temporaryCurrentDir) {
            Remove-Item -LiteralPath $temporaryCurrentDir -Recurse -Force
        }
        throw "Failed to update current launcher target. $($_.Exception.Message)"
    }

    if ($previousCurrentDir -and (Test-Path $previousCurrentDir)) {
        Remove-Item -LiteralPath $previousCurrentDir -Recurse -Force
    }

    Write-Launcher
    if (Add-UserPathEntry) {
        Write-Host "Added launcher directory to PATH: $BinDir"
    }
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
