param(
    [string]$Version = "latest",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$Repo = "JarsirLiu/CloudAgent"
$ScriptBaseUrl = if ($env:CLOUDAGENT_SCRIPT_BASE_URL) {
    $env:CLOUDAGENT_SCRIPT_BASE_URL
}
else {
    "https://github.com/$Repo/releases/latest/download"
}
$ScriptFallbackUrl = if ($env:CLOUDAGENT_SCRIPT_FALLBACK_URL) {
    $env:CLOUDAGENT_SCRIPT_FALLBACK_URL
}
else {
    "https://raw.githubusercontent.com/$Repo/main/scripts"
}
$DefaultInstallRoot = if ($IsWindows -and $env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA "CloudAgent" } else { Join-Path $HOME ".local/share/cloudagent" }
$LegacyInstallRoot = Join-Path $HOME ".local/lib/cloudagent"
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { $DefaultInstallRoot }
$CurrentDir = Join-Path $InstallRoot "current"
$SupportDir = Join-Path $CurrentDir "support"
$CurrentExe = Join-Path $CurrentDir "cloudagent.exe"
$CurrentNode = Join-Path $CurrentDir "node.exe"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "cloudagent-upgrade-$PID"
$script:LastDownloadStatusLength = 0
$script:CurlCommand = Get-Command curl.exe -ErrorAction SilentlyContinue
$script:StageTotal = 4

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

function Resolve-RequestedVersion {
    param([string]$Value)

    $normalized = if ($null -eq $Value) { "" } else { $Value.Trim() }
    if (-not $normalized) {
        return "latest"
    }

    return $normalized
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

            $releaseOrInstallDir = Split-Path -Parent $supportParent
            if ($releaseOrInstallDir) {
                $leafName = Split-Path -Leaf $releaseOrInstallDir
                if ($leafName -eq "releases" -or $leafName -eq "installs") {
                    $resolvedRoot = Split-Path -Parent $releaseOrInstallDir
                    return $resolvedRoot
                }
            }
        }
    }

    if ((-not $IsWindows) -and (Test-Path (Join-Path $LegacyInstallRoot "current"))) {
        return $LegacyInstallRoot
    }

    return $DefaultInstallRoot
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

function Stop-NodeIfRunning {
    if (-not (Test-Path $CurrentNode)) {
        return $false
    }

    $wasRunning = Test-NodeRunning
    if (-not $wasRunning) {
        return $false
    }

    Write-StageStart -Step 1 -Title "Stopping local node"
    $processIds = Get-ManagedProcessIds
    if ($processIds.Count -gt 0) {
        Stop-Process -Id $processIds -Force
    }
    Write-StageDone
    return $true
}

function Test-LocalInstallerAvailable {
    $localSupportScript = Join-Path $SupportDir "install.ps1"
    if (Test-Path $localSupportScript) {
        return $true
    }

    if ($PSScriptRoot) {
        $localScript = Join-Path $PSScriptRoot "install.ps1"
        if (Test-Path $localScript) {
            return $true
        }
    }

    return $false
}

function Get-LocalInstallerPath {
    $localSupportScript = Join-Path $SupportDir "install.ps1"
    if (Test-Path $localSupportScript) {
        return $localSupportScript
    }

    if ($PSScriptRoot) {
        $localScript = Join-Path $PSScriptRoot "install.ps1"
        if (Test-Path $localScript) {
            return $localScript
        }
    }

    return $null
}

function Assert-LocalInstallerAvailable {
    if (Get-LocalInstallerPath) {
        return
    }

    throw "Missing local installer support at $(Join-Path $SupportDir 'install.ps1'). Re-run the bootstrap installer to repair this installation."
}

function Test-UpgradeRestartNeeded {
    if (-not (Test-Path $CurrentNode)) {
        return $false
    }

    return Test-NodeRunning
}

function Start-NodeAfterUpgrade {
    if (-not (Test-Path $CurrentExe)) {
        throw "Upgrade completed but cloudagent.exe is missing from $CurrentDir"
    }

    Write-StageStart -Step 4 -Title "Restarting local node"
    & $CurrentExe start
    if ($LASTEXITCODE -ne 0) {
        throw "Upgrade installed successfully, but failed to restart the local node"
    }
    Write-StageDone
}

function Invoke-InstallScript {
    $requestedVersion = Resolve-RequestedVersion $Version

    $localInstaller = Get-LocalInstallerPath
    if ($localInstaller) {
        & $localInstaller -Version $requestedVersion -Force:$Force
        if ($LASTEXITCODE -ne 0) {
            throw "support install.ps1 failed with exit code $LASTEXITCODE"
        }
        return
    }

    throw "Missing local installer support at $(Join-Path $SupportDir 'install.ps1'). Re-run the bootstrap installer to repair this installation."
}

try {
    $InstallRoot = Resolve-InstallRoot
    $CurrentDir = Join-Path $InstallRoot "current"
    $SupportDir = Join-Path $CurrentDir "support"
    $CurrentExe = Join-Path $CurrentDir "cloudagent.exe"
    $CurrentNode = Join-Path $CurrentDir "node.exe"
    $restartNode = $false
    Assert-LocalInstallerAvailable
    if (Test-UpgradeRestartNeeded) {
        $restartNode = Stop-NodeIfRunning
    }
    else {
        Write-StageStart -Step 1 -Title "Checking local node"
        Write-StageDone -Detail "(not running)"
    }
    Write-StageStart -Step 3 -Title "Running installer"
    try {
        Invoke-InstallScript
        Write-StageDone
    }
    catch {
        if ($restartNode -and (Test-Path $CurrentExe)) {
            Write-StageStart -Step 4 -Title "Restoring local node"
            try {
                & $CurrentExe start
            }
            catch {
            }
            Write-StageDone -Detail "(best effort)"
        }
        throw
    }
    if ($restartNode) {
        Start-NodeAfterUpgrade
    }
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
