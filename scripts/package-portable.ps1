#requires -Version 5.1
<#
.SYNOPSIS
  Build SushiBox Inari and assemble a portable, copy-and-run bundle.

.DESCRIPTION
  Produces dist/SushiBox-Inari-Portable/ containing everything needed to run
  on a clean Windows machine (Server 2019 -> 2025) by copying the folder:

    Inari.exe          built release binary
    runtime/           nginx, php, mariadb, redis, adminer, webview2 (if fetched)
    flavors/           default flavor config
    scripts/           fetch-runtime.ps1 (to (re)download runtime if missing)
    sites/default/     starter web root
    README.txt         quick start

  Runtime binaries are included ONLY if already present in runtime/. Missing
  components are listed at the end so you can fetch them before shipping.

.PARAMETER Zip
  Also produce dist/SushiBox-Inari-Portable.zip

.EXAMPLE
  .\scripts\package-portable.ps1
  .\scripts\package-portable.ps1 -Zip
#>
param(
    [switch]$Zip
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$DistRoot = Join-Path $RepoRoot 'dist'
$OutName  = 'SushiBox-Inari-Portable'
$OutDir   = Join-Path $DistRoot $OutName

function Say($msg, $color = 'Gray') { Write-Host "  $msg" -ForegroundColor $color }

Write-Host "SushiBox Inari - portable packager" -ForegroundColor Cyan
Write-Host ("=" * 48)

# --- 1. Build release ------------------------------------------------------
Say "Building release binary..." 'Cyan'
& cargo build --release --manifest-path (Join-Path $RepoRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$ExeSrc = Join-Path $RepoRoot 'target\release\Inari.exe'
if (-not (Test-Path $ExeSrc)) { throw "Built exe not found: $ExeSrc" }
$CliSrc = Join-Path $RepoRoot 'target\release\inari-cli.exe'

# --- 2. Clean output -------------------------------------------------------
if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

# --- 3. Copy executables ---------------------------------------------------
Say "Copying Inari.exe (GUI)"
Copy-Item $ExeSrc (Join-Path $OutDir 'Inari.exe') -Force
if (Test-Path $CliSrc) {
    Say "Copying inari-cli.exe (console/automation)"
    Copy-Item $CliSrc (Join-Path $OutDir 'inari-cli.exe') -Force
}

# --- 4. Copy runtime (only populated components) ---------------------------
$runtimeSrc = Join-Path $RepoRoot 'runtime'
$runtimeOut = Join-Path $OutDir 'runtime'
New-Item -ItemType Directory -Path $runtimeOut -Force | Out-Null

# manifest always travels with the bundle
Copy-Item (Join-Path $runtimeSrc 'manifest.toml') (Join-Path $runtimeOut 'manifest.toml') -Force

$components = @('nginx','php','mysql','redis','adminer','webview2')
$included = @()
$missing  = @()
foreach ($c in $components) {
    $src = Join-Path $runtimeSrc $c
    $has = (Test-Path $src) -and (@(Get-ChildItem $src -Recurse -File -EA SilentlyContinue).Count -gt 0)
    if ($has) {
        Say "runtime/$c"
        Copy-Item $src (Join-Path $runtimeOut $c) -Recurse -Force
        $included += $c
    } else {
        $missing += $c
    }
}

# --- 4a. Adminer dev bootstrap --------------------------------------------
# Adminer is shipped as a downloaded single-file PHP app. For Inari we wrap it
# so the bundled local MariaDB is preselected (127.0.0.1:3307 / root / empty
# password), and patch a PHP 8 warning string Adminer 4.8.1 does not suppress.
$adminerOut = Join-Path $runtimeOut 'adminer'
if (Test-Path $adminerOut) {
    $adminerPhp  = Join-Path $adminerOut 'adminer.php'
    $adminerCore = Join-Path $adminerOut 'adminer-core.php'

    if ((Test-Path $adminerPhp) -and (-not (Test-Path $adminerCore))) {
        Move-Item $adminerPhp $adminerCore -Force
    }

    if (Test-Path $adminerCore) {
        $core = Get-Content $adminerCore -Raw
        $core = $core -replace 'Trying to access array offset on value of type null\|Undefined array key', 'Trying to access array offset on (value of type )?null|Undefined array key'
        [System.IO.File]::WriteAllText($adminerCore, $core, (New-Object System.Text.UTF8Encoding($false)))

        $wrapper = @'
<?php
/** Inari Adminer wrapper.
 * Local-only dev defaults for the bundled MariaDB:
 *   server: 127.0.0.1:3307
 *   user:   root
 *   pass:   empty
 */
function adminer_object() {
    class AdminerInari extends Adminer {
        function credentials() {
            return array('127.0.0.1:3307', 'root', '');
        }

        function login($login, $password) {
            return ($login === 'root');
        }
    }
    return new AdminerInari;
}

$_GET['server'] = $_GET['server'] ?? '127.0.0.1:3307';
$_GET['username'] = $_GET['username'] ?? 'root';
$_GET['db'] = $_GET['db'] ?? '';
$_REQUEST['server'] = $_GET['server'];
$_REQUEST['username'] = $_GET['username'];
$_REQUEST['db'] = $_GET['db'];
require __DIR__ . '/adminer-core.php';
'@
        [System.IO.File]::WriteAllText($adminerPhp, $wrapper, (New-Object System.Text.UTF8Encoding($false)))
        Say "Adminer wrapper prepared for Inari defaults" 'Green'
    }
}

# --- 4b. Prune the shipped runtime (source runtime/ stays intact) ----------
# The win is MySQL: portable MariaDB ships debug symbols, dev link libs, a
# template datadir, backup tooling, and ~25 maintenance CLIs a dev runtime
# never launches. We keep only what the app spawns plus the two clients a dev
# actually uses (mysql, mysqldump). The datadir is regenerated on first run by
# init_mysql_if_needed(), so a bundled data/ is pure weight.
function Get-DirMB($path) {
    if (-not (Test-Path $path)) { return 0 }
    [math]::Round((Get-ChildItem $path -Recurse -File -EA SilentlyContinue | Measure-Object Length -Sum).Sum / 1MB, 1)
}

$mysqlOut = Join-Path $runtimeOut 'mysql'
if (Test-Path $mysqlOut) {
    $before = Get-DirMB $mysqlOut
    Say "Trimming bundled MySQL ($before MB)..." 'Cyan'

    # 1. Drop the template datadir (recreated on first run).
    $dataDir = Join-Path $mysqlOut 'data'
    if (Test-Path $dataDir) { Remove-Item $dataDir -Recurse -Force }

    # 2. Drop debug symbols and dev link libraries everywhere under mysql/.
    Get-ChildItem $mysqlOut -Recurse -File -Include *.pdb, *.lib -EA SilentlyContinue |
        Remove-Item -Force -EA SilentlyContinue

    # 3. bin/: keep only the binaries the app spawns + the two dev clients.
    $keepBin = @(
        'mysqld.exe',            # the server (spawned)
        'mysql_install_db.exe',  # datadir init (spawned on first run)
        'mysqladmin.exe',        # graceful shutdown (spawned on stop)
        'mysql.exe',             # interactive client (dev convenience)
        'mysqldump.exe'          # manual backups (dev convenience)
    )
    $binDir = Join-Path $mysqlOut 'bin'
    if (Test-Path $binDir) {
        Get-ChildItem $binDir -File -EA SilentlyContinue | Where-Object {
            $_.Extension -eq '.exe' -and ($keepBin -notcontains $_.Name)
        } | Remove-Item -Force -EA SilentlyContinue
    }

    # 4. share/: keep charset/collation data + English error messages only.
    #    MariaDB refuses to start without charsets/ and an errmsg file, but the
    #    20+ localized message dirs and JDBC/Mongo connector jars are optional.
    $shareDir = Join-Path $mysqlOut 'share'
    if (Test-Path $shareDir) {
        $keepShareDirs  = @('charsets', 'english')
        Get-ChildItem $shareDir -Directory -EA SilentlyContinue | Where-Object {
            $keepShareDirs -notcontains $_.Name
        } | Remove-Item -Recurse -Force -EA SilentlyContinue
        # Drop connector jars and the large help-table seed SQL (not needed to run).
        Get-ChildItem $shareDir -File -Include *.jar, 'fill_help_tables.sql' -EA SilentlyContinue |
            Remove-Item -Force -EA SilentlyContinue
    }

    # 5. include/: C headers, only needed to compile against libmysql.
    $incDir = Join-Path $mysqlOut 'include'
    if (Test-Path $incDir) { Remove-Item $incDir -Recurse -Force }

    $after = Get-DirMB $mysqlOut
    Say ("MySQL trimmed: {0} MB -> {1} MB" -f $before, $after) 'Green'
}

# --- 5. Copy support folders ----------------------------------------------
Say "Copying flavors/ scripts/ sites/"
Copy-Item (Join-Path $RepoRoot 'flavors') (Join-Path $OutDir 'flavors') -Recurse -Force

# Legal: ship the app license + third-party notices with every bundle. The
# bundle redistributes GPL/BSD/OFL components, so these must travel with it.
foreach ($f in @('LICENSE', 'THIRD_PARTY.md')) {
    $p = Join-Path $RepoRoot $f
    if (Test-Path $p) { Copy-Item $p (Join-Path $OutDir $f) -Force }
}

New-Item -ItemType Directory -Path (Join-Path $OutDir 'scripts') -Force | Out-Null
foreach ($scriptName in @('fetch-runtime.ps1', 'release-check.ps1', 'smoke-desktop.ps1')) {
    $scriptPath = Join-Path $RepoRoot ("scripts\{0}" -f $scriptName)
    if (Test-Path $scriptPath) {
        Copy-Item $scriptPath (Join-Path $OutDir ("scripts\{0}" -f $scriptName)) -Force
    }
}

$siteSrc = Join-Path $RepoRoot 'sites\default'
$siteOut = Join-Path $OutDir 'sites\default'
New-Item -ItemType Directory -Path $siteOut -Force | Out-Null
if (Test-Path $siteSrc) {
    Copy-Item (Join-Path $siteSrc '*') $siteOut -Recurse -Force -EA SilentlyContinue
}
# Ensure a starter index exists
$indexPhp = Join-Path $siteOut 'index.php'
if (-not (Test-Path $indexPhp)) {
    '<?php phpinfo();' | Out-File -FilePath $indexPhp -Encoding utf8
}

# --- 6. README -------------------------------------------------------------
$readme = @"
SushiBox Inari - Portable

QUICK START
  1. Double-click Inari.exe  -> opens the control window (no console)
  2. Click Start All         -> launches nginx / php / mariadb / redis
  3. Web root: sites/default  (http://localhost:8080)

NO INSTALL REQUIRED. Copy this whole folder anywhere and run.

CLI (for automation / AI) — use inari-cli.exe (has console output):
  inari-cli.exe start | stop | restart | status

  (Inari.exe accepts the same commands but is a GUI binary with no console,
   so use inari-cli.exe when you need to see output.)

If services show "missing binary", fetch the runtime:
  powershell -ExecutionPolicy Bypass -File scripts\fetch-runtime.ps1

Windows Server 2019/2022: include runtime\webview2\ (Fixed Version) so the
window renders without the system Edge runtime. See runtime\manifest.toml.

LICENSE
  Inari's own code is MIT (see LICENSE). It bundles nginx, PHP, MariaDB, Redis,
  Adminer, WebView2 and the Be Vietnam Pro font, each under its own license -
  see THIRD_PARTY.md. The bundled MariaDB server is GPL-2.0; if you redistribute
  this bundle, the GPL source-availability terms apply.
"@
$readme | Out-File -FilePath (Join-Path $OutDir 'README.txt') -Encoding utf8

# --- 7. Report -------------------------------------------------------------
Write-Host ("=" * 48)
$bundleSize = (Get-ChildItem $OutDir -Recurse -File | Measure-Object Length -Sum).Sum
Say ("Bundle: {0}" -f $OutDir) 'Green'
Say ("Size  : {0:N1} MB" -f ($bundleSize / 1MB)) 'Green'
Say ("Runtime included: {0}" -f ($(if ($included) { $included -join ', ' } else { 'none' })))
if ($missing.Count -gt 0) {
    Say ("Runtime MISSING : {0}" -f ($missing -join ', ')) 'Yellow'
    Say "  -> run scripts\fetch-runtime.ps1 before shipping" 'Yellow'
}

if ($Zip) {
    $zipPath = Join-Path $DistRoot ($OutName + '.zip')
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    Say "Zipping..." 'Cyan'
    Compress-Archive -Path $OutDir -DestinationPath $zipPath -Force
    Say ("Zip   : {0} ({1:N1} MB)" -f $zipPath, ((Get-Item $zipPath).Length / 1MB)) 'Green'
}

Write-Host ("=" * 48)
Write-Host "Done." -ForegroundColor Green
