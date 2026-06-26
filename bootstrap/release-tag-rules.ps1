$ErrorActionPreference = "Stop"

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
