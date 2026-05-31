//! `inari-cli.exe` — console CLI for automation / AI.
//! The GUI is the separate Tauri app (Inari.exe).

use anyhow::Result;
use clap::Parser;
use inari_cli::{init_tracing, run_command, Cli, Commands};

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    // Default with no subcommand: show status (headless-friendly).
    run_command(cli.command.unwrap_or(Commands::Status))
}
