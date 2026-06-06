use anyhow::Result;
use inari_core::paths::InariPaths;
use inari_core::process::ServiceKind;
use inari_core::runtime::{stop_service, wait_for_exit};
use inari_core::state;

use crate::supervisor::{build_descriptors, load_config, read_pid, remove_pid, run_hooks};

pub async fn run() -> Result<()> {
    let paths = InariPaths::from_exe()?;
    let config = load_config(&paths);
    let descriptors = build_descriptors(&paths, &config);

    let port_for = |kind: &ServiceKind| match kind {
        ServiceKind::Nginx => config.ports.web,
        ServiceKind::Php   => 9000,
        ServiceKind::Mysql => config.ports.mysql,
        ServiceKind::Redis => config.ports.redis,
    };

    let mut stopped = 0usize;

    // Stop in reverse dependency order: nginx (front door) first, backends last.
    for kind in ServiceKind::stop_order() {
        if descriptors.iter().all(|d| &d.kind != kind) {
            continue;
        }
        if let Some(pid) = read_pid(&paths, kind) {
            if state::is_running(&paths, kind) {
                stop_service(&paths, kind, port_for(kind), config.mysql_password.as_deref());
                // Wait for the process to actually exit so its port is freed
                // (matters for an immediate restart). MariaDB shutdown can take
                // a moment; cap the wait so a stuck process can't hang us.
                wait_for_exit(pid, 5000);
                println!("  [STOP] {} (pid {pid})", kind.display_name());
                stopped += 1;
            } else {
                println!("  [GONE] {} — process not found, cleaned up", kind.display_name());
            }
            remove_pid(&paths, kind);
        } else {
            println!("  [----] {} — not running", kind.display_name());
        }
    }

    println!("\nStopped {stopped} service(s).");
    run_hooks(&config.hooks.on_stop);
    Ok(())
}
