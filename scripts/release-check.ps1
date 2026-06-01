#requires -Version 5.1
<#
.SYNOPSIS
  Run the repeatable SushiBox Inari beta release checklist.

.DESCRIPTION
  Verifies the repo is ready to package, runs the Rust and panel builds, then
  optionally assembles the portable bundle. This script is intentionally small
  and reproducible so every beta can be checked the same way.

.PARAMETER AllowDirty
  Continue even when the git working tree has uncommitted changes. Useful while
  validating a beta candidate before the final commit; omit for final release.

.PARAMETER Package
  Run scripts/package-portable.ps1 after tests/builds pass.

.PARAMETER Zip
  When used with -Package, also create the portable zip.

.EXAMPLE
  .\scripts\release-check.ps1
  .\scripts\release-check.ps1 -AllowDirty
  .\scripts\release-check.ps1 -Package -Zip
#>
param(
    [switch]$AllowDirty,
    [switch]$Package,
    [switch]$Zip
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot

function Step($msg) {
    Write-Host "`n==> $msg" -ForegroundColor Cyan
}

function Run($exe, [string[]]$argv, $cwd = $RepoRoot) {
    Push-Location $cwd
    try {
        & $exe @argv
        if ($LASTEXITCODE -ne 0) {
            throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

Step 'Checking git working tree'
$status = git -C $RepoRoot status --short
if ($status) {
    Write-Host $status -ForegroundColor Yellow
    if (-not $AllowDirty) {
        throw 'Working tree is dirty. Commit/stash changes or rerun with -AllowDirty for candidate validation.'
    }
}
else {
    Write-Host 'Working tree clean.' -ForegroundColor Green
}

Step 'Running Rust tests'
Run 'cargo' @('test')

Step 'Building Nuxt panel'
Run 'bun' @('run', 'build') (Join-Path $RepoRoot 'panel')

if ($Package) {
    Step 'Packaging portable bundle'
    $args = @('-ExecutionPolicy', 'Bypass', '-File', (Join-Path $RepoRoot 'scripts\package-portable.ps1'))
    if ($Zip) { $args += '-Zip' }
    Run 'powershell' $args
}
else {
    Write-Host "`nSkipping portable package. Add -Package or -Package -Zip when ready." -ForegroundColor DarkGray
}

Write-Host "`nRelease check passed." -ForegroundColor Green
