#requires -Version 5.1
<#
.SYNOPSIS
  Smoke-test a portable Inari bundle from a desktop/install folder.

.DESCRIPTION
  Starts Inari.exe if needed, waits for the local panel API, verifies settings
  save/config/status state sync, and optionally starts services + checks web and
  Adminer responses. Intended for repeatable beta/v0.9 readiness checks.

.PARAMETER InstallDir
  Path to the portable bundle folder.

.PARAMETER WebPort
  Web/nginx port to save through the Settings API and verify across state.

.PARAMETER StartServices
  Also start MariaDB, Redis, and nginx, then smoke web/Adminer HTTP responses.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\smoke-desktop.ps1 `
    -InstallDir C:\Users\QuangQuoc\Desktop\SushiBox-Inari-MUH5-Dev -WebPort 81 -StartServices
#>
param(
    [string]$InstallDir = "C:\Users\QuangQuoc\Desktop\SushiBox-Inari-MUH5-Dev",
    [int]$WebPort = 81,
    [switch]$StartServices
)

$ErrorActionPreference = 'Stop'
$Api = 'http://127.0.0.1:1788'

function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Fail($msg) { throw "SMOKE FAILED: $msg" }
function Post($path, $body = $null) {
    if ($null -eq $body) {
        Invoke-RestMethod "$Api$path" -Method Post
    } else {
        Invoke-RestMethod "$Api$path" -Method Post -Body ($body | ConvertTo-Json -Depth 8) -ContentType 'application/json'
    }
}
function PostJson($path, [string]$json) {
    Invoke-RestMethod "$Api$path" -Method Post -Body $json -ContentType 'application/json'
}

Step 'Starting / locating Inari panel API'
$exe = Join-Path $InstallDir 'Inari.exe'
if (-not (Test-Path $exe)) { Fail "Inari.exe not found at $exe" }
if (-not (Get-Process -Name Inari -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath $exe -WorkingDirectory $InstallDir
}

$config = $null
for ($i = 0; $i -lt 15; $i++) {
    try {
        $config = Invoke-RestMethod "$Api/api/config" -TimeoutSec 3
        break
    } catch {
        Start-Sleep -Seconds 1
    }
}
if ($null -eq $config) { Fail 'Panel API did not become ready on 127.0.0.1:1788' }
Write-Host "API ready. Current web port: $($config.ports.web)" -ForegroundColor Green

Step 'Saving settings and verifying canonical state sync'
$settings = Invoke-RestMethod "$Api/api/settings"
$siteRoot = if ($settings.sites -and $settings.sites.Count -gt 0) { $settings.sites[0].root } elseif ($config.sites -and $config.sites.Count -gt 0) { $config.sites[0].root } else { 'sites/default' }
$escapedRoot = $siteRoot.Replace('\', '\\')
$runAtStartup = if ($settings.run_at_startup) { 'true' } else { 'false' }
$startMinimized = if ($settings.start_minimized) { 'true' } else { 'false' }
$bodyJson = @"
{
  "ports": { "panel": null, "web": $WebPort, "mysql": 3307, "redis": 6380 },
  "sites": [{ "name": "default", "root": "$escapedRoot", "index": "index.php" }],
  "autostart": [],
  "run_at_startup": $runAtStartup,
  "start_minimized": $startMinimized
}
"@
$save = PostJson '/api/settings' $bodyJson
if (-not $save.ok) { Fail "Settings save failed: $($save.error)" }
if ($save.config.ports.web -ne $WebPort) { Fail "Save response config web=$($save.config.ports.web), expected $WebPort" }
if ($save.settings.ports.web -ne $WebPort) { Fail "Save response settings web=$($save.settings.ports.web), expected $WebPort" }

$config = Invoke-RestMethod "$Api/api/config"
if ($config.ports.web -ne $WebPort) { Fail "API config web=$($config.ports.web), expected $WebPort" }
$status = Invoke-RestMethod "$Api/api/status"
$nginx = $status.services | Where-Object { $_.kind -eq 'nginx' } | Select-Object -First 1
if ($null -eq $nginx) { Fail 'nginx missing from status response' }
if ($nginx.port -ne $WebPort) { Fail "status nginx port=$($nginx.port), expected $WebPort" }
Write-Host "State sync OK: settings/config/status web port = $WebPort" -ForegroundColor Green

if ($StartServices) {
    Step 'Starting services'
    foreach ($svc in @('mysql', 'redis', 'nginx')) {
        $res = Post "/api/services/$svc/start"
        if (-not $res.ok -and ($res.error -notmatch 'already running')) {
            Fail "$svc start failed: $($res.error)"
        }
        Write-Host "${svc}: $($res.ok) $($res.error)"
    }
    Start-Sleep -Seconds 2

    $status = Invoke-RestMethod "$Api/api/status"
    $running = $status.services | Where-Object { $_.state -ne 'running' }
    if ($running) { Fail "Some services are not running: $($running.kind -join ', ')" }

    Step 'Checking web and Adminer responses'
    $web = Invoke-WebRequest "http://127.0.0.1:$WebPort/" -UseBasicParsing -TimeoutSec 10
    if ($web.StatusCode -lt 200 -or $web.StatusCode -ge 500) { Fail "Web returned HTTP $($web.StatusCode)" }

    try {
        $adm = Invoke-WebRequest "http://127.0.0.1:$WebPort/_inari/adminer.php?server=127.0.0.1%3A3307&username=root" -UseBasicParsing -TimeoutSec 10
        $admBody = $adm.Content
    } catch {
        $resp = $_.Exception.Response
        if ($null -eq $resp) { Fail "Adminer request failed: $($_.Exception.Message)" }
        $sr = New-Object System.IO.StreamReader($resp.GetResponseStream())
        $admBody = $sr.ReadToEnd()
    }
    if ($admBody -match 'Warning|Fatal error|No input file|does not support accessing') {
        Fail 'Adminer response contains PHP/Adminer error text'
    }
    Write-Host 'Web + Adminer smoke OK' -ForegroundColor Green
}

Write-Host "`nSMOKE PASS" -ForegroundColor Green
