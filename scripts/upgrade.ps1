param(
    [string]$Version = "latest",
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$Repo = "JarsirLiu/CloudAgent"
$InstallRoot = if ($env:CLOUDAGENT_INSTALL_ROOT) { $env:CLOUDAGENT_INSTALL_ROOT } else { Join-Path $env:LOCALAPPDATA "CloudAgent" }
$CurrentDir = Join-Path $InstallRoot "current"
$CurrentExe = Join-Path $CurrentDir "cloudagent.exe"
$CurrentNode = Join-Path $CurrentDir "node.exe"
$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) "cloudagent-upgrade-$PID"

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
    $directory = Split-Path -Parent $OutFile
    if ($directory) {
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
    }

    $webClient = New-Object System.Net.WebClient
    try {
        foreach ($entry in $Headers.GetEnumerator()) {
            $webClient.Headers[[string]$entry.Key] = [string]$entry.Value
        }

        $downloadCompleted = [System.Threading.ManualResetEvent]::new($false)
        $downloadError = [ref]$null
        $progressHandler = [System.Net.DownloadProgressChangedEventHandler]{
            param($sender, $eventArgs)
            Write-DownloadStatus -Label $Label -DownloadedBytes $eventArgs.BytesReceived -TotalBytes $eventArgs.TotalBytesToReceive
        }
        $completedHandler = [System.ComponentModel.AsyncCompletedEventHandler]{
            param($sender, $eventArgs)
            if ($eventArgs.Error) {
                $downloadError.Value = $eventArgs.Error
            }
            $downloadCompleted.Set() | Out-Null
        }
        $webClient.add_DownloadProgressChanged($progressHandler)
        $webClient.add_DownloadFileCompleted($completedHandler)

        try {
            $webClient.DownloadFileAsync([Uri]$Uri, $OutFile)
            while (-not $downloadCompleted.WaitOne(250)) {
                Start-Sleep -Milliseconds 50
            }
            if ($downloadError.Value) {
                throw $downloadError.Value
            }
            Write-Progress -Activity $Label -Completed
        }
        finally {
            $webClient.remove_DownloadProgressChanged($progressHandler)
            $webClient.remove_DownloadFileCompleted($completedHandler)
            $downloadCompleted.Dispose()
        }
    }
    finally {
        $webClient.Dispose()
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

    Write-Host "Stopping local node before upgrade"
    $processIds = Get-ManagedProcessIds
    if ($processIds.Count -gt 0) {
        Stop-Process -Id $processIds -Force
    }
    return $true
}

function Start-NodeAfterUpgrade {
    if (-not (Test-Path $CurrentExe)) {
        throw "Upgrade completed but cloudagent.exe is missing from $CurrentDir"
    }

    Write-Host "Starting local node after upgrade"
    & $CurrentExe start
    if ($LASTEXITCODE -ne 0) {
        throw "Upgrade installed successfully, but failed to restart the local node"
    }
}

function Invoke-InstallScript {
    if ($PSScriptRoot) {
        $localScript = Join-Path $PSScriptRoot "install.ps1"
        if (Test-Path $localScript) {
            & $localScript -Version $Version -Force:$Force
            return
        }
    }

    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    $installScript = Join-Path $tempRoot "install.ps1"
    Invoke-DownloadFile `
        -Uri "https://raw.githubusercontent.com/$Repo/main/scripts/install.ps1" `
        -Headers @{ "User-Agent" = "cloudagent-upgrade" } `
        -OutFile $installScript `
        -Label "Downloading installer script"
    & $installScript -Version $Version -Force:$Force
}

try {
    $restartNode = Stop-NodeIfRunning
    Write-Host "Installing updated CloudAgent version"
    Invoke-InstallScript
    if ($restartNode) {
        Start-NodeAfterUpgrade
    }
}
finally {
    if (Test-Path $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force
    }
}
