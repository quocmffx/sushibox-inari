use inari_core::{
    config::InariConfig,
    paths::InariPaths,
    process::{ServiceDescriptor, ServiceKind, ServiceStatus},
    state,
};
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
// PID helpers — thin re-exports of the shared state module so the CLI, panel
// and API all read/write liveness identically.
// ---------------------------------------------------------------------------

pub use inari_core::state::{read_pid, remove_pid, write_pid};

// ---------------------------------------------------------------------------
// Status check — delegates to the shared, PID-reuse-aware state module.
// ---------------------------------------------------------------------------

pub fn check_status(
    paths: &InariPaths,
    descriptors: &[ServiceDescriptor],
) -> Vec<(ServiceKind, ServiceStatus)> {
    let kinds: Vec<ServiceKind> = descriptors.iter().map(|d| d.kind.clone()).collect();
    let available: std::collections::HashMap<ServiceKind, bool> = descriptors
        .iter()
        .map(|d| (d.kind.clone(), d.is_available()))
        .collect();
    state::statuses(paths, &kinds, |k| available.get(k).copied().unwrap_or(false))
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
