//! Single source of truth for service liveness + PID file handling.
//!
//! Every "is it running?" decision in Inari (CLI status, panel `/api/status`,
//! start/stop guards) routes through here so they can never disagree.
//!
//! Why this exists: a bare `sys.process(pid).is_some()` only proves *some*
//! process owns that PID right now. Windows recycles PIDs aggressively, so a
//! dead service's PID can be reused by an unrelated process and naively report
//! as "running". We additionally match the process's executable name against
//! the service's known binary, which makes a recycled PID read as stopped (and
//! its stale PID file gets cleaned up).

use std::fs;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::paths::InariPaths;
use crate::process::{ServiceKind, ServiceStatus};

// ---------------------------------------------------------------------------
// PID file I/O
// ---------------------------------------------------------------------------

pub fn read_pid(paths: &InariPaths, kind: &ServiceKind) -> Option<u32> {
    fs::read_to_string(paths.pid_file(kind.name()))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn write_pid(paths: &InariPaths, kind: &ServiceKind, pid: u32) -> anyhow::Result<()> {
    fs::create_dir_all(&paths.data)?;
    fs::write(paths.pid_file(kind.name()), pid.to_string())
        .map_err(|e| anyhow::anyhow!("Cannot write PID file for {}: {e}", kind.name()))
}

pub fn remove_pid(paths: &InariPaths, kind: &ServiceKind) {
    let _ = fs::remove_file(paths.pid_file(kind.name()));
}

// ---------------------------------------------------------------------------
// Liveness
// ---------------------------------------------------------------------------

/// True if `pid` currently belongs to a process whose executable matches the
/// service's expected binary. `sys` must already have that PID refreshed.
fn pid_matches_service(sys: &System, pid: u32, kind: &ServiceKind) -> bool {
    match sys.process(Pid::from_u32(pid)) {
        Some(proc) => proc
            .exe()
            .and_then(|p| p.file_name())
            .map(|n| n.eq_ignore_ascii_case(kind.exe_file_name()))
            // If the OS won't reveal the exe path (rare; permissions), fall back
            // to "the PID exists" rather than falsely reporting stopped.
            .unwrap_or(true),
        None => false,
    }
}

/// Refresh just the given PIDs, requesting the executable path (needed so
/// `pid_matches_service` can confirm identity and defeat PID reuse).
fn refresh_pids(sys: &mut System, pids: &[Pid]) {
    if pids.is_empty() {
        return;
    }
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(pids),
        false,
        ProcessRefreshKind::new().with_exe(UpdateKind::Always),
    );
}

/// Resolve one service's status, cleaning up a stale PID file if the recorded
/// PID is gone or has been recycled by an unrelated process.
pub fn status_of(paths: &InariPaths, kind: &ServiceKind, binary_present: bool) -> ServiceStatus {
    if !binary_present {
        return ServiceStatus::MissingBinary;
    }
    let Some(pid) = read_pid(paths, kind) else {
        return ServiceStatus::Stopped;
    };

    let mut sys = System::new();
    refresh_pids(&mut sys, &[Pid::from_u32(pid)]);

    if pid_matches_service(&sys, pid, kind) {
        ServiceStatus::Running { pid }
    } else {
        // Dead or recycled — drop the stale file so future reads are clean.
        remove_pid(paths, kind);
        ServiceStatus::Stopped
    }
}

/// Batch status for several services in a single process snapshot. Cleans up
/// any stale PID files encountered. Order of the result matches `kinds`.
pub fn statuses(
    paths: &InariPaths,
    kinds: &[ServiceKind],
    binary_present: impl Fn(&ServiceKind) -> bool,
) -> Vec<(ServiceKind, ServiceStatus)> {
    // Collect the PIDs we actually need, then refresh only those — no full
    // system enumeration just to check a handful of services.
    let recorded: Vec<(ServiceKind, Option<u32>)> = kinds
        .iter()
        .map(|k| (k.clone(), read_pid(paths, k)))
        .collect();

    let pids: Vec<Pid> = recorded
        .iter()
        .filter_map(|(_, p)| p.map(Pid::from_u32))
        .collect();

    let mut sys = System::new();
    refresh_pids(&mut sys, &pids);

    recorded
        .into_iter()
        .map(|(kind, pid)| {
            let status = if !binary_present(&kind) {
                ServiceStatus::MissingBinary
            } else if let Some(pid) = pid {
                if pid_matches_service(&sys, pid, &kind) {
                    ServiceStatus::Running { pid }
                } else {
                    remove_pid(paths, &kind);
                    ServiceStatus::Stopped
                }
            } else {
                ServiceStatus::Stopped
            };
            (kind, status)
        })
        .collect()
}

/// Whether a service is currently running (binary assumed present).
pub fn is_running(paths: &InariPaths, kind: &ServiceKind) -> bool {
    matches!(status_of(paths, kind, true), ServiceStatus::Running { .. })
}

/// After spawning, confirm the process is still alive after a short grace
/// period. Catches services that exit immediately (port already bound, missing
/// DLL, corrupt datadir) so callers don't report a crashed process as started.
///
/// Returns true if the PID still matches the service after `grace_ms`.
pub fn confirm_alive(pid: u32, kind: &ServiceKind, grace_ms: u64) -> bool {
    std::thread::sleep(std::time::Duration::from_millis(grace_ms));
    let mut sys = System::new();
    refresh_pids(&mut sys, &[Pid::from_u32(pid)]);
    pid_matches_service(&sys, pid, kind)
}
