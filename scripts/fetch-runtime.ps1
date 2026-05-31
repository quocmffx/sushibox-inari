#requires -Version 5.1
<#
.SYNOPSIS
  Fetch and extract SushiBox Inari runtime binaries per runtime/manifest.toml.

.DESCRIPTION
  Downloads each component's archive, verifies sha256 when provided,
  extracts into its dest (honouring strip / single_file), and skips
  components that have no URL. Runtime binaries are NOT committed to git.

.PARAMETER Component
  Optional. Fetch only this component (nginx|php|mysql|redis|adminer).
  Default: every component in the manifest.

.PARAMETER Force
  Re-download and overwrite even if the dest is already populated.

.EXAMPLE
  .\scripts\fetch-runtime.ps1
  .\scripts\fetch-runtime.ps1 -Component php
  .\scripts\fetch-runtime.ps1 -Force
#>
param(
    [string]$Component,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

# Repo root = parent of this script's folder
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Manifest = Join-Path $RepoRoot 'runtime\manifest.toml'
$TmpDir   = Join-Path $env:TEMP 'inari-runtime-fetch'

if (-not (Test-Path -LiteralPath $Manifest)) {
    throw "Manifest not found: $Manifest"
}
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

# --- minimal TOML parser (flat [section] key = value) ----------------------
function Read-Manifest {
    param([string]$Path)
    $sections = [ordered]@{}
    $current  = $null
    foreach ($raw in Get-Content -LiteralPath $Path) {
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        if ($line -match '^\[(.+)\]$') {
            $current = $Matches[1].Trim()
            $sections[$current] = @{}
            continue
        }
        if ($current -and $line -match '^([A-Za-z_]+)\s*=\s*(.+)$') {
            $key = $Matches[1]
            $val = $Matches[2].Trim()
            if ($val -notmatch '^"') { $val = ($val -split '#', 2)[0].Trim() }
            if     ($val -match '^"(.*)"$') { $val = $Matches[1] }
            elseif ($val -eq 'true')        { $val = $true }
            elseif ($val -eq 'false')       { $val = $false }
            $sections[$current][$key] = $val
        }
    }
    return $sections
}

# --- helpers ----------------------------------------------------------------
function Test-Sha {
    param([string]$File, [string]$Expected)
    if ([string]::IsNullOrWhiteSpace($Expected)) {
        Write-Host "    [warn] no sha256 in manifest - skipping integrity check" -ForegroundColor Yellow
        return $true
    }
    $actual = (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $Expected.ToLower()) {
        Write-Host "    [FAIL] sha256 mismatch" -ForegroundColor Red
        Write-Host "           expected $Expected"
        Write-Host "           actual   $actual"
        return $false
    }
    Write-Host "    [ok] sha256 verified" -ForegroundColor Green
    return $true
}

function Get-Component {
    param([string]$Name, [hashtable]$Spec)

    $url = [string]$Spec['url']
    if ([string]::IsNullOrWhiteSpace($url)) {
        Write-Host "  [SKIP] $Name - no url in manifest" -ForegroundColor DarkGray
        return
    }

    $dest   = Join-Path $RepoRoot ([string]$Spec['dest'] -replace '/', '\')
    $single = [bool]$Spec['single_file']

    $populated = if ($single) {
        Test-Path -LiteralPath $dest
    } else {
        (Test-Path -LiteralPath $dest) -and
        @(Get-ChildItem -LiteralPath $dest -ErrorAction SilentlyContinue).Count -gt 0
    }
    if ($populated -and -not $Force) {
        Write-Host "  [HAVE] $Name - already present (use -Force to refresh)" -ForegroundColor DarkGray
        return
    }

    Write-Host "  [GET ] $Name <- $url" -ForegroundColor Cyan
    $file = Join-Path $TmpDir (Split-Path $url -Leaf)
    Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing

    if (-not (Test-Sha -File $file -Expected ([string]$Spec['sha256']))) {
        throw "$Name integrity check failed"
    }

    if ($single) {
        New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
        Copy-Item -LiteralPath $file -Destination $dest -Force
        Write-Host "  [DONE] $Name -> $dest" -ForegroundColor Green
        return
    }

    $strip = 0
    if ($Spec.ContainsKey('strip')) { $strip = [int]$Spec['strip'] }

    $work = Join-Path $TmpDir "$Name-extract"
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
    Expand-Archive -LiteralPath $file -DestinationPath $work -Force

    New-Item -ItemType Directory -Path $dest -Force | Out-Null
    if ($strip -ge 1) {
        $top = Get-ChildItem -LiteralPath $work -Directory | Select-Object -First 1
        if (-not $top) { throw "${Name}: strip=$strip but archive has no top-level folder" }
        Get-ChildItem -LiteralPath $top.FullName -Force |
            Copy-Item -Destination $dest -Recurse -Force
    } else {
        Get-ChildItem -LiteralPath $work -Force |
            Copy-Item -Destination $dest -Recurse -Force
    }
    Write-Host "  [DONE] $Name -> $dest" -ForegroundColor Green
}

# --- main -------------------------------------------------------------------
Write-Host "SushiBox Inari - runtime fetch"
Write-Host ("=" * 48)

$manifest = Read-Manifest -Path $Manifest
$targets  = if ($Component) {
    if (-not $manifest.Contains($Component)) { throw "Unknown component: $Component" }
    @($Component)
} else {
    @($manifest.Keys)
}

foreach ($name in $targets) {
    Get-Component -Name $name -Spec $manifest[$name]
}

Write-Host ("=" * 48)
Write-Host "Done. Runtime binaries are git-ignored and not committed." -ForegroundColor Green
