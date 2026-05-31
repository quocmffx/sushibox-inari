use anyhow::Result;
use inari_core::paths::InariPaths;
use inari_core::runtime::stop_service;
use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::supervisor::{build_descriptors, load_config, read_pid, remove_pid, run_hooks};

pub async fn run() -> Result<()> {
    let paths = InariPaths::from_exe()?;
    let config = load_config(&paths);
    let descriptors = build_descriptors(&paths, &config);

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let port_for = |kind: &inari_core::process::ServiceKind| match kind {
        inari_core::process::ServiceKind::Nginx => config.ports.web,
        inari_core::process::ServiceKind::Php   => 9000,
        inari_core::process::ServiceKind::Mysql => config.ports.mysql,
        inari_core::process::ServiceKind::Redis => config.ports.redis,
    };

    let mut stopped = 0usize;

    for desc in &descriptors {
        if let Some(pid) = read_pid(&paths, &desc.kind) {
            if sys.process(Pid::from_u32(pid)).is_some() {
                stop_service(&paths, &desc.kind, port_for(&desc.kind));
                println!("  [STOP] {} (pid {pid})", desc.kind.display_name());
                stopped += 1;
            } else {
                println!("  [GONE] {} — process not found, cleaned up", desc.kind.display_name());
            }
            remove_pid(&paths, &desc.kind);
        } else {
            println!("  [----] {} — not running", desc.kind.display_name());
        }
    }

    println!("\nStopped {stopped} service(s).");
    run_hooks(&config.hooks.on_stop);
    Ok(())
}
