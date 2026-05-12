param(
    [string]$Version = "latest",
    [switch]$Force
)

$scriptPath = Join-Path $PSScriptRoot "install.ps1"
& $scriptPath -Version $Version -Force:$Force
