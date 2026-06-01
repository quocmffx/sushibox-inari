use anyhow::Result;
use inari_core::{
    job::JobObject, paths::InariPaths, process::ServiceKind, process::spawn_service, state,
};
use tracing::warn;

use crate::supervisor::{
    build_descriptors, generate_nginx_conf, generate_php_ini, init_mysql_if_needed, load_config, remove_pid,
    run_hooks, write_pid,
};

/// Grace period after spawn before we trust a service as "up". Long enough to
/// catch an immediate exit (port clash, missing DLL, bad datadir), short enough
/// not to drag out a multi-service start.
const LIVENESS_GRACE_MS: u64 = 400;

pub async fn run() -> Result<()> {
    let paths = InariPaths::from_exe()?;
    let config = load_config(&paths);
    let descriptors = build_descriptors(&paths, &config);

    std::fs::create_dir_all(&paths.data)?;
    std::fs::create_dir_all(&paths.logs)?;

    // Initialise MariaDB datadir on first run (no-op if already done).
    if paths.mysql_exe().exists() {
        if let Err(e) = init_mysql_if_needed(&paths) {
            println!("  [WARN] MariaDB init failed: {e}");
        }
    }

    if paths.nginx_exe().exists() {
        generate_nginx_conf(&paths, &config)?;
    }

    if paths.php_exe().exists() {
        generate_php_ini(&paths)?;
    }

    let job = JobObject::new()?;
    let mut running = Vec::new();
    let mut started = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;

    // Start in dependency order (backends → PHP → nginx) so nginx never comes
    // up before the FastCGI/DB endpoints it proxies to.
    for kind in ServiceKind::start_order() {
        let Some(desc) = descriptors.iter().find(|d| &d.kind == kind) else {
            continue;
        };
        if !desc.is_available() {
            println!("  [SKIP] {} — missing binary", desc.kind.display_name());
            skipped += 1;
            continue;
        }
        match spawn_service(desc) {
            Ok(svc) => {
                let pid = svc.pid();
                if let Err(e) = job.assign(pid) {
                    warn!("Job assign failed for {}: {e}", desc.kind.name());
                }
                // Confirm it didn't exit immediately before claiming success.
                if state::confirm_alive(pid, &desc.kind, LIVENESS_GRACE_MS) {
                    write_pid(&paths, &desc.kind, pid)?;
                    println!("  [ OK] {} started (pid {pid})", desc.kind.display_name());
                    started += 1;
                    running.push(svc);
                } else {
                    println!(
                        "  [FAIL] {} exited immediately — check logs (port in use? missing dependency?)",
                        desc.kind.display_name()
                    );
                    remove_pid(&paths, &desc.kind);
                    failed += 1;
                }
            }
            Err(e) => {
                println!("  [ERR] {} — {e}", desc.kind.display_name());
                failed += 1;
            }
        }
    }

    println!();
    if started == 0 {
        println!("No services started. Place runtime binaries in runtime/ and retry.");
        return Ok(());
    }
    println!(
        "Started {started} service(s), skipped {skipped} (missing binary), failed {failed}."
    );
    println!("Panel : http://127.0.0.1:{}", config.ports.panel);
    println!("Web   : http://127.0.0.1:{}", config.ports.web);
    println!("\nPress Ctrl+C to stop all services.");

    run_hooks(&config.hooks.on_start);

    tokio::signal::ctrl_c().await?;
    println!("\nShutting down...");

    for svc in &mut running {
        let _ = svc.kill();
    }
    for desc in &descriptors {
        remove_pid(&paths, &desc.kind);
    }
    run_hooks(&config.hooks.on_stop);
    drop(job);
    println!("Done.");
    Ok(())
}
