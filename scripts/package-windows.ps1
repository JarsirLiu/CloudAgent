param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Version = "manual",
    [string]$Profile = "release",
    [string]$Package = "cloudagent",
    [string]$Output
)

$ErrorActionPreference = "Stop"

if (-not $Output) {
    $normalizedVersion = if ($Version.StartsWith("v")) { $Version.Substring(1) } else { $Version }
    $Output = "dist/cloudagent-$normalizedVersion-$Target.msi"
}

$targetBinDir = "target/$Target/$Profile"
$wixInput = "packaging/windows/wix/main.wxs"

if (-not (Test-Path $wixInput)) {
    throw "missing WiX template at $wixInput"
}

if (-not (Test-Path $targetBinDir)) {
    throw "missing target bin dir at $targetBinDir; build binaries first"
}

New-Item -ItemType Directory -Path (Split-Path -Parent $Output) -Force | Out-Null

Write-Host "Building MSI with WiX template: $wixInput"
Write-Host "Target: $Target"
Write-Host "Profile: $Profile"
Write-Host "Output: $Output"

cargo wix `
  --package $Package `
  --input $wixInput `
  --target $Target `
  --profile $Profile `
  --no-build `
  --target-bin-dir $targetBinDir `
  --output $Output
