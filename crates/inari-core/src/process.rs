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

    /// Executable file name this service runs as, used to verify that a PID we
    /// recorded still belongs to *our* process and wasn't recycled by Windows.
    pub fn exe_file_name(&self) -> &'static str {
        match self {
            Self::Nginx => "nginx.exe",
            Self::Php   => "php-cgi.exe",
            Self::Mysql => "mysqld.exe",
            Self::Redis => "redis-server.exe",
        }
    }

    /// Stable order for display/status (groups by how users think of the stack).
    pub fn all() -> &'static [ServiceKind] {
        &[Self::Nginx, Self::Php, Self::Mysql, Self::Redis]
    }

    /// Services shown to users. PHP-CGI is an implementation detail of nginx in
    /// this stack, so it is still managed internally but not presented as a
    /// standalone service.
    pub fn public() -> &'static [ServiceKind] {
        &[Self::Nginx, Self::Mysql, Self::Redis]
    }

    /// Dependency-aware launch order. nginx proxies PHP to 127.0.0.1:9000 and
    /// serves the site, so the backends (MariaDB, Redis) and the PHP FastCGI
    /// worker must be up *before* nginx — otherwise the first requests hit a
    /// dead upstream. Starting nginx last closes that window.
    pub fn start_order() -> &'static [ServiceKind] {
        &[Self::Mysql, Self::Redis, Self::Php, Self::Nginx]
    }

    /// Reverse of `start_order`: stop nginx first so it stops accepting traffic
    /// before the backends it depends on go away.
    pub fn stop_order() -> &'static [ServiceKind] {
        &[Self::Nginx, Self::Php, Self::Redis, Self::Mysql]
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

// ---------------------------------------------------------------------------
// Port conflict diagnostics (simple PowerShell probe for actionable error)
// ---------------------------------------------------------------------------

/// Check if a TCP port is already bound on localhost. Returns Some((pid, exe_name))
/// if in use, None otherwise. Used to provide clearer start error messages.
pub fn port_in_use(port: u16) -> Option<(u32, String)> {
    // Use PowerShell Get-NetTCPConnection to find owner PID (fast, works on Win10+)
    let script = format!(
        "$conn = Get-NetTCPConnection -LocalPort {} -ErrorAction SilentlyContinue | Select-Object -First 1; if ($conn) {{ $proc = Get-Process -Id $conn.OwningProcess -ErrorAction SilentlyContinue; Write-Output \"$($conn.OwningProcess) $($proc.ProcessName)\" }}",
        port
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = text.trim().split_whitespace().collect();
            if parts.len() == 2 {
                let pid: Option<u32> = parts[0].parse().ok();
                if let Some(p) = pid {
                    return Some((p, parts[1].to_string()));
                }
            }
            None
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// start_order must place nginx last (it depends on the PHP/DB upstreams)
    /// and include every service exactly once.
    #[test]
    fn start_order_brings_nginx_up_last() {
        let order = ServiceKind::start_order();
        assert_eq!(order.last(), Some(&ServiceKind::Nginx));
        assert_eq!(order.len(), ServiceKind::all().len());
        for k in ServiceKind::all() {
            assert!(order.contains(k), "{k:?} missing from start_order");
        }
    }

    /// stop_order must be the exact reverse of start_order: nginx (front door)
    /// goes down first, the backends it relies on last.
    #[test]
    fn stop_order_is_reverse_of_start_order() {
        let start = ServiceKind::start_order();
        let stop: Vec<_> = ServiceKind::stop_order().to_vec();
        let rev_start: Vec<_> = start.iter().rev().cloned().collect();
        assert_eq!(stop, rev_start);
    }

    /// Each service must know the on-disk binary used to confirm PID identity.
    #[test]
    fn exe_file_names_are_distinct_and_exe() {
        let names: Vec<&str> = ServiceKind::all().iter().map(|k| k.exe_file_name()).collect();
        for n in &names {
            assert!(n.ends_with(".exe"), "{n} should be a Windows exe name");
        }
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "exe names must be unique per service");
    }

    #[test]
    fn public_services_hide_php() {
        assert!(ServiceKind::all().contains(&ServiceKind::Php));
        assert!(!ServiceKind::public().contains(&ServiceKind::Php));
        assert_eq!(ServiceKind::public(), &[ServiceKind::Nginx, ServiceKind::Mysql, ServiceKind::Redis]);
    }
}
