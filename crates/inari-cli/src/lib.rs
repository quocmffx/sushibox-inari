//! Shared library for the `inari-cli` console binary (automation / AI).
//!
//! The GUI is a separate Tauri app (see `src-tauri/`). This crate is the
//! headless control surface: start/stop/restart/status, panel server, menu.

pub mod commands;
pub mod supervisor;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "inari-cli", about = "Inari (SushiBox) — runtime control (CLI)", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Clone, Copy)]
pub enum Commands {
    /// Start all available runtime services (foreground, Ctrl+C to stop)
    Start,
    /// Stop all running runtime services
    Stop,
    /// Restart all runtime services
    Restart,
    /// Show status of all runtime services
    Status,
    /// Start the panel HTTP server + open the system browser
    Panel,
    /// Open the interactive terminal menu
    Menu,
}

/// Initialise tracing once.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("inari=info,warn")),
        )
        .init();
}

/// Run a command on a fresh tokio runtime.
pub fn run_command(command: Commands) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        match command {
            Commands::Start   => commands::start::run().await,
            Commands::Stop    => commands::stop::run().await,
            Commands::Restart => commands::restart::run().await,
            Commands::Status  => commands::status::run().await,
            Commands::Panel   => commands::panel::run().await,
            Commands::Menu    => commands::menu::run().await,
        }
    })
}
