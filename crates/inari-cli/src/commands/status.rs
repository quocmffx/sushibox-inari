use anyhow::Result;
use inari_core::{paths::InariPaths, process::{ServiceKind, ServiceStatus}};

use crate::supervisor::{build_descriptors, check_status, load_config};

pub async fn run() -> Result<()> {
    let paths  = InariPaths::from_exe()?;
    let config = load_config(&paths);
    let descs  = build_descriptors(&paths, &config);
    let stats  = check_status(&paths, &descs)
        .into_iter()
        .filter(|(kind, _)| ServiceKind::public().contains(kind))
        .collect::<Vec<_>>();

    println!("Inari — service status");
    println!("{}", "─".repeat(48));

    for (kind, status) in &stats {
        let icon = match status {
            ServiceStatus::Running { .. } => "●",
            ServiceStatus::Stopped        => "○",
            ServiceStatus::MissingBinary  => "✗",
            ServiceStatus::Error { .. }   => "!",
        };
    println!("  {icon}  {:<14}  {status}", format!("{} {}", kind.display_name(), kind.version()));
    }

    println!("{}", "─".repeat(48));
    println!(
        "Ports  panel={} web={} mysql={} redis={}",
        config.ports.panel, config.ports.web,
        config.ports.mysql, config.ports.redis,
    );
    Ok(())
}
