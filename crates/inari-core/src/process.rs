use std::path::PathBuf;
use std::process::{Child, Command};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

// ---------------------------------------------------------------------------
// Service identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Nginx,
    Php,
    Mysql,
    Redis,
}

impl ServiceKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Nginx => "nginx",
            Self::Php   => "php",
            Self::Mysql => "mysql",
            Self::Redis => "redis",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Nginx => "Nginx",
            Self::Php   => "PHP-CGI",
            Self::Mysql => "MariaDB",
            Self::Redis => "Redis",
        }
    }

    /// Bundled version, kept in sync with runtime/manifest.toml (source of truth
    /// for what fetch-runtime.ps1 downloads). These are pinned compatibility
    /// choices (e.g. MariaDB must stay 10.3 for the muh5 stack).
    pub fn version(&self) -> &'static str {
        match self {
            Self::Nginx => "1.18.0",
            Self::Php   => "8.4.21",
            Self::Mysql => "10.3.39",
            Self::Redis => "5.0.14.1",
        }
    }

    pub fn all() -> &'static [ServiceKind] {
        &[Self::Nginx, Self::Php, Self::Mysql, Self::Redis]
    }
}

// ---------------------------------------------------------------------------
// Descriptor (what to launch)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ServiceDescriptor {
    pub kind: ServiceKind,
    pub exe:  PathBuf,
    pub args: Vec<String>,
    pub env:  Vec<(String, String)>,
    pub cwd:  Option<PathBuf>,
}

impl ServiceDescriptor {
    pub fn is_available(&self) -> bool {
        self.exe.exists()
    }
}

// ---------------------------------------------------------------------------
// Running handle
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct RunningService {
    pub kind:  ServiceKind,
    pub child: Child,
}

impl RunningService {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(self.child.try_wait()?)
    }

    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("Failed to kill process")?;
        Ok(())
    }
}

pub fn spawn_service(desc: &ServiceDescriptor) -> Result<RunningService> {
    info!("Spawning {} from {:?}", desc.kind.name(), desc.exe);
    let mut cmd = Command::new(&desc.exe);
    cmd.args(&desc.args);
    for (k, v) in &desc.env {
        cmd.env(k, v);
    }
    if let Some(cwd) = &desc.cwd {
        cmd.current_dir(cwd);
    }

    // On Windows, suppress the console window and break out of any inherited
    // Job Object so services don't flash a terminal or get killed by the
    // parent's job handle closing (e.g. Tauri/WebView2 runs in a job).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW        = 0x08000000
        // CREATE_BREAKAWAY_FROM_JOB = 0x01000000
        cmd.creation_flags(0x08000000 | 0x01000000);
    }

    let child = cmd.spawn().with_context(|| {
        format!("Failed to spawn {} at {:?}", desc.kind.name(), desc.exe)
    })?;
    Ok(RunningService { kind: desc.kind.clone(), child })
}

// ---------------------------------------------------------------------------
// Status (serialisable for API + CLI)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServiceStatus {
    Running { pid: u32 },
    Stopped,
    MissingBinary,
    Error { message: String },
}

impl std::fmt::Display for ServiceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running { pid }    => write!(f, "running (pid {pid})"),
            Self::Stopped            => write!(f, "stopped"),
            Self::MissingBinary      => write!(f, "missing binary"),
            Self::Error { message }  => write!(f, "error: {message}"),
        }
    }
}
