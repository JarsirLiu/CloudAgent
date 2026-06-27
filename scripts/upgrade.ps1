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
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$CurrentDir = Join-Path $InstallRoot "current"
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

    if ($PSScriptRoot) {
        $localScript = Join-Path $PSScriptRoot "install.ps1"
        if (Test-Path $localScript) {
            & $localScript -Version $requestedVersion -Force:$Force
            if ($LASTEXITCODE -ne 0) {
                throw "local install.ps1 failed with exit code $LASTEXITCODE"
            }
            return
        }
    }

    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    $installScript = Join-Path $tempRoot "install.ps1"
    Write-StageStart -Step 2 -Title "Downloading installer script"
    $downloaded = $false
    foreach ($baseUrl in @($ScriptBaseUrl, $ScriptFallbackUrl)) {
        if (-not $baseUrl) {
            continue
        }

        $scriptUrl = ($baseUrl.TrimEnd('/') + '/install.ps1')
        try {
            Invoke-DownloadFile `
                -Uri $scriptUrl `
                -Headers @{ "User-Agent" = "cloudagent-upgrade" } `
                -OutFile $installScript `
                -Label "Downloading installer script"
            $downloaded = $true
            break
        }
        catch {
            if (Test-Path $installScript) {
                Remove-Item -LiteralPath $installScript -Force
            }
        }
    }

    if (-not $downloaded) {
        throw "failed to download install.ps1 from configured script sources"
    }
    Write-StageDone
    & $installScript -Version $requestedVersion -Force:$Force
    if ($LASTEXITCODE -ne 0) {
        throw "install.ps1 failed with exit code $LASTEXITCODE"
    }
}

try {
    $restartNode = $false
    if (Test-UpgradeRestartNeeded) {
        $restartNode = Stop-NodeIfRunning
    }
    else {
        Write-StageStart -Step 1 -Title "Checking local node"
        Write-StageDone -Detail "(not running)"
    }
    Write-StageStart -Step 3 -Title "Running installer"
    Invoke-InstallScript
    Write-StageDone
    if ($restartNode) {
        Start-NodeAfterUpgrade
    }
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
