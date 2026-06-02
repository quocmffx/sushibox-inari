//! Shared runtime service logic: descriptor building + nginx.conf generation.
//! Used by both the CLI supervisor and the API control endpoints so they
//! launch services identically.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::debug;

use crate::config::InariConfig;
use crate::paths::InariPaths;
use crate::process::{ServiceDescriptor, ServiceKind};

// ---------------------------------------------------------------------------
// Service descriptors
// ---------------------------------------------------------------------------

/// Build the launch descriptor for a single service kind.
pub fn descriptor_for(
    kind: &ServiceKind,
    paths: &InariPaths,
    config: &InariConfig,
) -> ServiceDescriptor {
    match kind {
        ServiceKind::Nginx => ServiceDescriptor {
            kind: ServiceKind::Nginx,
            exe:  paths.nginx_exe(),
            args: vec![
                "-c".to_string(), paths.nginx_conf().to_string_lossy().into_owned(),
                "-p".to_string(), paths.nginx.to_string_lossy().into_owned(),
            ],
            env: vec![],
            cwd: Some(paths.nginx.clone()),
        },
        ServiceKind::Php => ServiceDescriptor {
            kind: ServiceKind::Php,
            exe:  paths.php_exe(),
            args: vec![
                "-c".to_string(), paths.php_ini().to_string_lossy().into_owned(),
                "-b".to_string(), "127.0.0.1:9000".to_string(),
            ],
            env: vec![],
            cwd: Some(paths.php.clone()),
        },
        ServiceKind::Mysql => ServiceDescriptor {
            kind: ServiceKind::Mysql,
            exe:  paths.mysql_exe(),
            args: vec![
                format!("--datadir={}", paths.mysql_datadir().to_string_lossy()),
                format!("--port={}", config.ports.mysql),
                "--bind-address=127.0.0.1".to_string(),
                "--skip-networking=0".to_string(),
                "--console".to_string(),
            ],
            env: vec![],
            cwd: Some(paths.mysql.clone()),
        },
        ServiceKind::Redis => ServiceDescriptor {
            kind: ServiceKind::Redis,
            exe:  paths.redis_exe(),
            args: vec![
                "--port".to_string(), config.ports.redis.to_string(),
                "--bind".to_string(), "127.0.0.1".to_string(),
            ],
            env: vec![],
            cwd: Some(paths.redis.clone()),
        },
    }
}

/// Build descriptors for all known services.
pub fn build_descriptors(paths: &InariPaths, config: &InariConfig) -> Vec<ServiceDescriptor> {
    ServiceKind::all()
        .iter()
        .map(|kind| descriptor_for(kind, paths, config))
        .collect()
}

// ---------------------------------------------------------------------------
// nginx.conf generation
// ---------------------------------------------------------------------------

/// Write nginx.conf from the flavor template, overwriting any existing file.
pub fn generate_nginx_conf(paths: &InariPaths, config: &InariConfig) -> Result<()> {
    fs::create_dir_all(&paths.config)?;

    let site_root = config.sites.first()
        .map(|s| paths.base.join(&s.root))
        .unwrap_or_else(|| paths.sites.join("default"));

    // nginx refuses to start if its log dir or document root are missing,
    // so ensure they exist on a fresh deployment (panel start path).
    fs::create_dir_all(&paths.logs)?;
    fs::create_dir_all(&site_root)?;

    let fwd = |p: &Path| p.to_string_lossy().replace('\\', "/");

    let tpl = config.nginx_template.as_deref().unwrap_or(DEFAULT_NGINX_TPL);
    let conf = tpl
        .replace("{nginx_dir}", &fwd(&paths.nginx))
        .replace("{logs_dir}",  &fwd(&paths.logs))
        .replace("{site_root}", &fwd(&site_root))
        .replace("{adminer_dir}", &fwd(&paths.adminer_dir()))
        .replace("{adminer_php}", &fwd(&paths.adminer_php()))
        .replace("{web_port}",  &config.ports.web.to_string());

    fs::write(paths.nginx_conf(), conf)
        .context("Failed to write nginx.conf")?;
    debug!("nginx.conf written to {:?}", paths.nginx_conf());
    Ok(())
}

/// Regenerate nginx.conf from the template.
/// The conf embeds absolute paths tied to the install location, so callers
/// regenerate it on every start to stay correct for portable / copied deploys.

// ---------------------------------------------------------------------------
// php.ini generation
// ---------------------------------------------------------------------------

/// Write a dev-tuned php.ini, overwriting any existing file.
///
/// php-cgi ships no active php.ini, so without this PHP runs with zero
/// extensions, errors hidden, no timezone, and tiny upload limits — useless
/// for the muh5 dev stack. We generate one with absolute extension_dir (so it
/// stays correct for portable/copied installs) and a sensible dev default set.
pub fn generate_php_ini(paths: &InariPaths) -> Result<()> {
    fs::create_dir_all(&paths.config)?;

    let ext_dir = paths.php.join("ext");
    let fwd = |p: &Path| p.to_string_lossy().replace('\\', "/");

    // Only enable extensions that actually exist in this build's ext/ dir, so a
    // slimmer PHP package never produces "cannot load extension" startup spam.
    let wanted = [
        "opcache", "curl", "fileinfo", "gd", "intl", "mbstring", "exif",
        "mysqli", "openssl", "pdo_mysql", "pdo_sqlite", "sqlite3",
        "sockets", "sodium", "zip",
    ];
    let mut ext_lines = String::new();
    for name in wanted {
        let dll = ext_dir.join(format!("php_{name}.dll"));
        if dll.exists() {
            // opcache is a zend_extension; the rest are plain extensions.
            if name == "opcache" {
                ext_lines.push_str(&format!("zend_extension=php_{name}.dll\n"));
            } else {
                ext_lines.push_str(&format!("extension=php_{name}.dll\n"));
            }
        }
    }

    let conf = PHP_INI_TPL
        .replace("{ext_dir}", &fwd(&ext_dir))
        .replace("{extensions}", ext_lines.trim_end());

    fs::write(paths.php_ini(), conf).context("Failed to write php.ini")?;
    debug!("php.ini written to {:?}", paths.php_ini());
    Ok(())
}

// ---------------------------------------------------------------------------
// MariaDB datadir initialisation
// ---------------------------------------------------------------------------
// MariaDB datadir initialisation
// ---------------------------------------------------------------------------

/// Run `mysql_install_db.exe` once when the datadir has never been initialised.
/// Safe to call on every start — no-ops if the `mysql/` schema dir already exists.
pub fn init_mysql_if_needed(paths: &InariPaths) -> Result<()> {
    let datadir = paths.mysql_datadir();

    if datadir.join("mysql").exists() {
        return Ok(());
    }

    let install_exe = paths.mysql.join("bin").join("mysql_install_db.exe");
    if !install_exe.exists() {
        tracing::warn!(
            "mysql_install_db.exe not found at {:?} — skipping datadir init",
            install_exe
        );
        return Ok(());
    }

    fs::create_dir_all(&datadir)?;
    println!("  [INIT] Initialising MariaDB datadir (first run, please wait)...");

    let status = std::process::Command::new(&install_exe)
        .args([&format!("--datadir={}", datadir.to_string_lossy())])
        .current_dir(&paths.mysql)
        .status()
        .with_context(|| format!("Failed to run {:?}", install_exe))?;

    if !status.success() {
        anyhow::bail!("mysql_install_db.exe exited with {}", status);
    }

    println!("  [INIT] MariaDB datadir ready.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Default nginx template
// ---------------------------------------------------------------------------

const DEFAULT_NGINX_TPL: &str = r#"worker_processes 1;
error_log "{logs_dir}/nginx-error.log";

events {
    worker_connections 1024;
}

http {
    include "{nginx_dir}/conf/mime.types";
    default_type application/octet-stream;
    access_log "{logs_dir}/nginx-access.log";
    sendfile off;

    server {
        listen {web_port};
        server_name localhost;
        root "{site_root}";
        index index.php index.html;

        # Front-controller friendly fallback for Laravel and other PHP apps.
        # Static files are served directly; unknown routes fall through to index.php.
        location / {
            try_files $uri $uri/ /index.php?$query_string;
        }

        # Bundled Adminer shortcut. This keeps the user's web root clean while
        # making the database UI available whenever runtime/adminer/ is included
        # in the portable bundle. The Inari wrapper pre-fills default credentials.
        location /_inari/ {
            alias "{adminer_dir}/";
            index index.php;
            location ~ \.php$ {
                fastcgi_pass 127.0.0.1:9000;
                fastcgi_index index.php;
                fastcgi_param SCRIPT_FILENAME $request_filename;
                include "{nginx_dir}/conf/fastcgi_params";
            }
        }

        location ~ \.php$ {
            fastcgi_pass 127.0.0.1:9000;
            fastcgi_index index.php;
            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;
            include "{nginx_dir}/conf/fastcgi_params";
        }
    }
}
"#;

// ---------------------------------------------------------------------------
// Default php.ini template (dev-tuned)
// ---------------------------------------------------------------------------

const PHP_INI_TPL: &str = r#"; SushiBox Inari — generated php.ini (dev profile).
; Regenerated on every PHP start; edit flavor/settings, not this file.

[PHP]
extension_dir = "{ext_dir}"

; Dev visibility: surface every error instead of white screens.
display_errors = On
display_startup_errors = On
error_reporting = E_ALL
log_errors = On

; Sensible dev limits (bigger than stock so uploads/imports don't choke).
memory_limit = 256M
max_execution_time = 120
upload_max_filesize = 64M
post_max_size = 64M
max_input_vars = 5000
default_charset = "UTF-8"
date.timezone = "UTC"

; cgi/fastcgi correctness for nginx.
cgi.fix_pathinfo = 1
cgi.force_redirect = 0

; Extensions (only those present in this build are listed).
{extensions}

[opcache]
; On for realistic perf, but revalidate fast so code edits show immediately.
opcache.enable = 1
opcache.enable_cli = 0
opcache.validate_timestamps = 1
opcache.revalidate_freq = 0
"#;

// ---------------------------------------------------------------------------
// Graceful service stop
// ---------------------------------------------------------------------------

/// Stop a service by PID. MariaDB gets a graceful `mysqladmin shutdown` first
/// (hard-killing mysqld forces crash recovery and risks datadir corruption);
/// all other services are killed directly. Returns true if a stop was issued.
pub fn stop_service(paths: &InariPaths, kind: &ServiceKind, port: u16) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    if *kind == ServiceKind::Nginx {
        return stop_nginx_processes(paths);
    }

    if *kind == ServiceKind::Mysql {
        let admin = paths.mysqladmin_exe();
        if admin.exists() {
            let ok = std::process::Command::new(&admin)
                .args([
                    "--protocol=tcp".to_string(),
                    "--host=127.0.0.1".to_string(),
                    format!("--port={port}"),
                    "--user=root".to_string(),
                    "shutdown".to_string(),
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                return true;
            }
            tracing::warn!("mysqladmin shutdown failed; falling back to kill");
        }
    }

    // Fallback / non-mysql: hard kill via PID — but only if the recorded PID
    // still belongs to *our* service. Windows recycles PIDs, so killing a bare
    // PID from a stale file could take down an unrelated process.
    if let Some(pid) = std::fs::read_to_string(paths.pid_file(kind.name()))
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        let mut sys = System::new();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
            false,
            ProcessRefreshKind::new().with_exe(UpdateKind::Always),
        );
        if let Some(proc) = sys.process(Pid::from_u32(pid)) {
            let matches = proc
                .exe()
                .and_then(|p| p.file_name())
                .map(|n| n.eq_ignore_ascii_case(kind.exe_file_name()))
                .unwrap_or(true);
            if matches {
                proc.kill();
                return true;
            }
        }
    }
    false
}

/// Stop every nginx process that belongs to this Inari instance. nginx on
/// Windows can leave a master/worker pair behind, and tracking only one PID lets
/// an orphan keep the web port bound while Inari thinks a different PID is live.
#[cfg(windows)]
fn stop_nginx_processes(paths: &InariPaths) -> bool {
    let nginx_exe = paths.nginx_exe().to_string_lossy().to_ascii_lowercase();
    let nginx_conf = paths.nginx_conf().to_string_lossy().replace('\\', "/").to_ascii_lowercase();
    let nginx_root = paths.nginx.to_string_lossy().replace('\\', "/").to_ascii_lowercase();

    let script = format!(
        "$exe = '{}'; $conf = '{}'; $root = '{}'; \
         $procs = Get-CimInstance Win32_Process -Filter \"Name = 'nginx.exe'\" | Where-Object {{ \
         $_.ExecutablePath -and ($_.ExecutablePath.ToLowerInvariant() -eq $exe) -and \
         $_.CommandLine -and (($normalized = $_.CommandLine.Replace('\\','/').ToLowerInvariant()).Contains($conf)) -and \
         $normalized.Contains($root) }}; \
         $count = 0; foreach ($p in $procs) {{ Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue; $count++ }}; $count",
        ps_single_quote(&nginx_exe),
        ps_single_quote(&nginx_conf),
        ps_single_quote(&nginx_root),
    );

    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|s| s.trim().parse::<usize>().ok())
        .map(|count| count > 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn stop_nginx_processes(_paths: &InariPaths) -> bool {
    false
}

#[cfg(windows)]
fn ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

/// Poll until a PID is gone (max ~3s). Used by restart so the port is released
/// before respawning, instead of guessing with a fixed sleep.
pub fn wait_for_exit(pid: u32, max_ms: u64) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let step = 100u64;
    let mut waited = 0u64;
    while waited < max_ms {
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), false);
        if sys.process(Pid::from_u32(pid)).is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(step));
        waited += step;
    }
}
