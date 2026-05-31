use std::fs;

use anyhow::{Context, Result};
use inari_core::{
    config::InariConfig,
    paths::InariPaths,
    process::{ServiceDescriptor, ServiceKind, ServiceStatus},
};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tracing::warn;

// Shared runtime logic now lives in inari-core; re-export so existing
// CLI call sites (start.rs, status.rs) keep importing from supervisor.
pub use inari_core::runtime::{build_descriptors, generate_nginx_conf, generate_php_ini, init_mysql_if_needed};

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

pub fn load_config(paths: &InariPaths) -> InariConfig {
    let flavor_path = paths.default_flavor();
    if flavor_path.exists() {
        match inari_lua::load_flavor(&flavor_path) {
            Ok(cfg) => return cfg,
            Err(e)  => warn!("Failed to load flavor: {e}; using defaults"),
        }
    }
    InariConfig::default()
}

// ---------------------------------------------------------------------------
// PID helpers
// ---------------------------------------------------------------------------

pub fn write_pid(paths: &InariPaths, kind: &ServiceKind, pid: u32) -> Result<()> {
    fs::create_dir_all(&paths.data)?;
    fs::write(paths.pid_file(kind.name()), pid.to_string())
        .with_context(|| format!("Cannot write PID file for {}", kind.name()))
}

pub fn read_pid(paths: &InariPaths, kind: &ServiceKind) -> Option<u32> {
    fs::read_to_string(paths.pid_file(kind.name()))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn remove_pid(paths: &InariPaths, kind: &ServiceKind) {
    let _ = fs::remove_file(paths.pid_file(kind.name()));
}

// ---------------------------------------------------------------------------
// Status check
// ---------------------------------------------------------------------------

pub fn check_status(
    paths: &InariPaths,
    descriptors: &[ServiceDescriptor],
) -> Vec<(ServiceKind, ServiceStatus)> {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    descriptors.iter().map(|desc| {
        let status = if !desc.is_available() {
            ServiceStatus::MissingBinary
        } else if let Some(pid) = read_pid(paths, &desc.kind) {
            if sys.process(Pid::from_u32(pid)).is_some() {
                ServiceStatus::Running { pid }
            } else {
                remove_pid(paths, &desc.kind);
                ServiceStatus::Stopped
            }
        } else {
            ServiceStatus::Stopped
        };
        (desc.kind.clone(), status)
    }).collect()
}

// ---------------------------------------------------------------------------
// Lifecycle hooks
// ---------------------------------------------------------------------------

/// Run each hook command synchronously, logging success/failure.
/// Hooks are plain shell commands from the Lua flavor config.
pub fn run_hooks(hooks: &[String]) {
    for hook in hooks {
        if hook.trim().is_empty() {
            continue;
        }
        #[cfg(windows)]
        let result = std::process::Command::new("cmd").args(["/c", hook]).status();
        #[cfg(not(windows))]
        let result = std::process::Command::new("sh").args(["-c", hook]).status();

        match result {
            Ok(s) if s.success() => tracing::info!("Hook ok: {hook}"),
            Ok(s)  => tracing::warn!("Hook exited {s}: {hook}"),
            Err(e) => tracing::warn!("Hook failed to spawn ({e}): {hook}"),
        }
    }
}

// ---------------------------------------------------------------------------
// MariaDB datadir initialisation — moved to inari-core::runtime
// ---------------------------------------------------------------------------
