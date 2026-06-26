param(
    [string]$Tag,
    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

. "$PSScriptRoot/release-tag-rules.ps1"

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
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

    Write-Host "validate-release-tag.ps1 self-test passed"
    exit 0
}

if (-not $Tag) {
    throw "missing release tag"
}

if (-not (Test-SemVerTag $Tag)) {
    throw "invalid release tag: $Tag"
}
