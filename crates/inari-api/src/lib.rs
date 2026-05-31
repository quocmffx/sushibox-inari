use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use inari_core::{
    InariConfig, InariPaths,
    job::JobObject,
    process::{ServiceKind, spawn_service},
    runtime::{descriptor_for, generate_nginx_conf, generate_php_ini, init_mysql_if_needed, stop_service, wait_for_exit},
    settings::Settings,
};
use mime_guess::from_path;
use rust_embed::RustEmbed;
use serde_json::json;
use sysinfo::{Pid, ProcessesToUpdate, System};
use tower_http::cors::CorsLayer;

// ---------------------------------------------------------------------------
// Embedded panel assets
// ---------------------------------------------------------------------------

#[derive(RustEmbed)]
#[folder = "../../panel/dist"]
struct PanelAssets;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct AppState {
    paths:    InariPaths,
    /// Live, effective config = flavor.lua + settings.json overlay.
    /// Mutable because the GUI can edit settings at runtime.
    config:   Mutex<InariConfig>,
    /// Base config from flavor.lua only (settings re-applied on top of this).
    base:     InariConfig,
    activity: Mutex<VecDeque<String>>,
    job:      Option<JobObject>,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn start_server(port: u16, paths: InariPaths, config: InariConfig) -> Result<()> {
    // Load settings.json overlay and apply onto flavor config.
    let settings = Settings::load(&paths.data);
    let effective = settings.apply_to(config.clone());

    let state = Arc::new(AppState {
        paths,
        config: Mutex::new(effective),
        base: config,
        activity: Mutex::new(VecDeque::with_capacity(50)),
        job: JobObject::new().ok(),
    });

    let app = Router::new()
        .route("/api/status",  get(api_status))
        .route("/api/config",  get(api_config))
        .route("/api/activity", get(api_activity))
        .route("/api/settings", get(api_get_settings).post(api_set_settings))
        .route("/api/services/:kind/start",   post(api_service_start))
        .route("/api/services/:kind/stop",    post(api_service_stop))
        .route("/api/services/:kind/restart", post(api_service_restart))
        .route("/api/open/:target", post(api_open))
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Panel listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Static file handler
// ---------------------------------------------------------------------------

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match PanelAssets::get(path) {
        Some(file) => {
            let mime = from_path(path).first_or_octet_stream();
            let body: Vec<u8> = file.data.into_owned();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string())], body).into_response()
        }
        None => match PanelAssets::get("index.html") {
            Some(file) => {
                let body: Vec<u8> = file.data.into_owned();
                ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
            }
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

// ---------------------------------------------------------------------------
// API — service control
// ---------------------------------------------------------------------------

/// Core start logic, shared by the HTTP handler and in-process autostart.
/// Returns `(ok, message)`.
fn start_service_inner(
    kind: &ServiceKind,
    paths: &InariPaths,
    config: &InariConfig,
    job: &Option<JobObject>,
) -> (bool, String) {
    if let Some(pid) = read_pid(paths, kind) {
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), false);
        if sys.process(Pid::from_u32(pid)).is_some() {
            return (false, format!("already running (pid {pid})"));
        }
        remove_pid(paths, kind);
    }
    let desc = descriptor_for(kind, paths, config);
    if !desc.is_available() {
        return (false, "binary not found".to_string());
    }
    if *kind == ServiceKind::Nginx {
        if let Err(e) = generate_nginx_conf(paths, config) {
            return (false, e.to_string());
        }
    }
    if *kind == ServiceKind::Php {
        if let Err(e) = generate_php_ini(paths) {
            return (false, e.to_string());
        }
    }
    if *kind == ServiceKind::Mysql {
        if let Err(e) = init_mysql_if_needed(paths) {
            return (false, e.to_string());
        }
    }
    match spawn_service(&desc) {
        Ok(svc) => {
            let pid = svc.pid();
            if let Some(j) = job {
                let _ = j.assign(pid);
            }
            drop(svc); // Child::drop() does NOT kill the process
            match write_pid(paths, kind, pid) {
                Ok(_)  => (true, format!("{} started (pid {pid})", kind.display_name())),
                Err(e) => (false, e.to_string()),
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

/// Start services by name directly in-process (used by autostart at launch).
/// Skips unknown names silently. Runs synchronously — call from a bg thread.
///
/// NOTE: No Job Object here — a local JobObject would be dropped at the end of
/// this function, killing every process it just assigned. The API server's
/// AppState holds the long-lived job; autostart processes are cleaned up by
/// Windows when the parent (Inari.exe) exits.
pub fn start_services_direct(kinds: &[String], paths: &InariPaths, config: &InariConfig) {
    let no_job: Option<JobObject> = None;
    for kind_str in kinds {
        let kind = match parse_kind(kind_str) {
            Some(k) => k,
            None => {
                tracing::warn!("autostart: unknown service '{kind_str}', skipping");
                continue;
            }
        };
        let (ok, msg) = start_service_inner(&kind, paths, config, &no_job);
        if ok {
            tracing::info!("autostart: {msg}");
        } else {
            tracing::warn!("autostart: {kind_str} failed — {msg}");
        }
    }
}

async fn api_service_start(
    Path(kind_str): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let kind = match parse_kind(&kind_str) {
        Some(k) => k,
        None => return axum::Json(json!({"ok": false, "error": "unknown service"})),
    };
    let config = state.config.lock().unwrap().clone();
    let (ok, msg) = start_service_inner(&kind, &state.paths, &config, &state.job);
    if ok {
        push_activity(&state, msg.clone());
        axum::Json(json!({"ok": true, "pid": msg}))
    } else {
        axum::Json(json!({"ok": false, "error": msg}))
    }
}

async fn api_service_stop(
    Path(kind_str): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let kind = match parse_kind(&kind_str) {
        Some(k) => k,
        None => return axum::Json(json!({"ok": false, "error": "unknown service"})),
    };
    let pid = match read_pid(&state.paths, &kind) {
        Some(p) => p,
        None => return axum::Json(json!({"ok": false, "error": "not running"})),
    };
    let port = { service_port(&kind, &state.config.lock().unwrap()) };
    stop_service(&state.paths, &kind, port);
    remove_pid(&state.paths, &kind);
    push_activity(&state, format!("{} stopped", kind.display_name()));
    let _ = pid;
    axum::Json(json!({"ok": true}))
}

async fn api_service_restart(
    Path(kind_str): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let kind = match parse_kind(&kind_str) {
        Some(k) => k,
        None => return axum::Json(json!({"ok": false, "error": "unknown service"})),
    };
    if let Some(pid) = read_pid(&state.paths, &kind) {
        let port = { service_port(&kind, &state.config.lock().unwrap()) };
        stop_service(&state.paths, &kind, port);
        wait_for_exit(pid, 3000);
        remove_pid(&state.paths, &kind);
    }
    let desc = {
        let config = state.config.lock().unwrap();
        descriptor_for(&kind, &state.paths, &config)
    };
    if !desc.is_available() {
        return axum::Json(json!({"ok": false, "error": "binary not found"}));
    }
    if kind == ServiceKind::Nginx {
        let config = state.config.lock().unwrap();
        if let Err(e) = generate_nginx_conf(&state.paths, &config) {
            return axum::Json(json!({"ok": false, "error": e.to_string()}));
        }
    }
    if kind == ServiceKind::Php {
        if let Err(e) = generate_php_ini(&state.paths) {
            return axum::Json(json!({"ok": false, "error": e.to_string()}));
        }
    }
    if kind == ServiceKind::Mysql {
        if let Err(e) = init_mysql_if_needed(&state.paths) {
            return axum::Json(json!({"ok": false, "error": e.to_string()}));
        }
    }
    match spawn_service(&desc) {
        Ok(svc) => {
            let pid = svc.pid();
            if let Some(job) = &state.job {
                let _ = job.assign(pid);
            }
            drop(svc);
            match write_pid(&state.paths, &kind, pid) {
                Ok(_)  => {
                    push_activity(&state, format!("{} restarted (pid {pid})", kind.display_name()));
                    axum::Json(json!({"ok": true, "pid": pid}))
                }
                Err(e) => axum::Json(json!({"ok": false, "error": e.to_string()})),
            }
        }
        Err(e) => axum::Json(json!({"ok": false, "error": e.to_string()})),
    }
}

// ---------------------------------------------------------------------------
// API — status + config
// ---------------------------------------------------------------------------

async fn api_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let pid_map: Vec<(ServiceKind, Option<u32>)> = ServiceKind::all()
        .iter()
        .map(|kind| (kind.clone(), read_pid(&state.paths, kind)))
        .collect();

    let sysinfo_pids: Vec<Pid> = pid_map
        .iter()
        .filter_map(|(_, p)| p.map(Pid::from_u32))
        .collect();

    let mut sys = System::new();
    if !sysinfo_pids.is_empty() {
        sys.refresh_processes(ProcessesToUpdate::Some(&sysinfo_pids), false);
    }

    let services: Vec<_> = pid_map
        .iter()
        .map(|(kind, pid)| {
            let running = pid
                .map(|p| sys.process(Pid::from_u32(p)).is_some())
                .unwrap_or(false);
            json!({
                "kind":    kind.name(),
                "name":    kind.display_name(),
                "version": kind.version(),
                "state":   if running { "running" } else { "stopped" },
                "pid":     pid,
            })
        })
        .collect();

    axum::Json(json!({ "services": services }))
}

async fn api_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config = state.config.lock().unwrap();
    axum::Json(json!({
        "flavor": config.flavor,
        "ports": {
            "panel": config.ports.panel,
            "web":   config.ports.web,
            "mysql": config.ports.mysql,
            "redis": config.ports.redis,
        },
        "sites": config.sites.iter().map(|s| json!({
            "name": s.name,
            "root": s.root,
        })).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// API — settings (GUI-editable config overlay)
// ---------------------------------------------------------------------------

async fn api_get_settings(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let settings = Settings::load(&state.paths.data);
    axum::Json(json!(settings))
}

async fn api_set_settings(
    State(state): State<Arc<AppState>>,
    axum::Json(settings): axum::Json<Settings>,
) -> impl IntoResponse {
    // Persist to settings.json
    if let Err(e) = settings.save(&state.paths.data) {
        return axum::Json(json!({"ok": false, "error": e.to_string()}));
    }
    // Reconcile the Windows "run at startup" registry entry with the setting.
    // This only controls whether Inari launches at boot, not which services run.
    if let Err(e) = inari_core::startup::apply(settings.run_at_startup) {
        tracing::warn!("run_at_startup apply failed: {e}");
    }
    // Re-apply onto base flavor config → update live effective config.
    let effective = settings.apply_to(state.base.clone());
    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = effective;
    }
    push_activity(&state, "settings updated".to_string());
    axum::Json(json!({"ok": true, "note": "Restart affected services to apply ports/sites."}))
}

async fn api_activity(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let log = state.activity.lock().unwrap();
    axum::Json(json!({ "entries": log.iter().collect::<Vec<_>>() }))
}

fn push_activity(state: &AppState, msg: String) {
    let mut log = state.activity.lock().unwrap();
    if log.len() >= 50 { log.pop_front(); }
    log.push_back(format!("[{}] {}", now_ts(), msg));
}

fn now_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    format!("{:02}:{:02}:{:02}", (s % 86400) / 3600, (s % 3600) / 60, s % 60)
}

async fn api_open(
    Path(target): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // "web" opens the running site in the system browser; "repo" opens the
    // project's GitHub page; the rest open folders.
    if target == "web" || target == "repo" {
        let url = if target == "repo" {
            "https://github.com/quocmffx/sushibox-inari".to_string()
        } else {
            let config = state.config.lock().unwrap();
            format!("http://localhost:{}", config.ports.web)
        };
        return match open_url(&url) {
            Ok(_) => {
                push_activity(&state, format!("opened {url}"));
                axum::Json(json!({"ok": true, "url": url}))
            }
            Err(e) => axum::Json(json!({"ok": false, "error": e.to_string()})),
        };
    }

    let path = match target.as_str() {
        "site" => {
            let config = state.config.lock().unwrap();
            config
                .sites
                .first()
                .map(|s| state.paths.base.join(&s.root))
                .unwrap_or_else(|| state.paths.sites.clone())
        }
        "config" => state.paths.config.clone(),
        "logs"   => state.paths.logs.clone(),
        "data"   => state.paths.data.clone(),
        _ => return axum::Json(json!({"ok": false, "error": "unknown target"})),
    };
    match open_path(&path) {
        Ok(_) => {
            push_activity(&state, format!("opened {}", target));
            axum::Json(json!({"ok": true, "path": path.to_string_lossy()}))
        }
        Err(e) => axum::Json(json!({"ok": false, "error": e.to_string()})),
    }
}

fn open_path(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    r.map(|_| ()).map_err(|e| anyhow::anyhow!("Cannot open {}: {e}", path.display()))
}

/// Open a URL in the system default browser (not the Tauri webview).
fn open_url(url: &str) -> Result<()> {
    #[cfg(windows)]
    // `explorer <url>` hands the URL to the default browser. `start` would need a
    // shell; explorer avoids spawning cmd and is reliable for http(s) URLs.
    let r = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(not(windows))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.map(|_| ()).map_err(|e| anyhow::anyhow!("Cannot open {url}: {e}"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_kind(s: &str) -> Option<ServiceKind> {
    match s {
        "nginx" => Some(ServiceKind::Nginx),
        "php"   => Some(ServiceKind::Php),
        "mysql" => Some(ServiceKind::Mysql),
        "redis" => Some(ServiceKind::Redis),
        _       => None,
    }
}

/// Port a service listens on, from config (php-cgi is fixed at 9000).
fn service_port(kind: &ServiceKind, config: &InariConfig) -> u16 {
    match kind {
        ServiceKind::Nginx => config.ports.web,
        ServiceKind::Php   => 9000,
        ServiceKind::Mysql => config.ports.mysql,
        ServiceKind::Redis => config.ports.redis,
    }
}

fn read_pid(paths: &InariPaths, kind: &ServiceKind) -> Option<u32> {
    std::fs::read_to_string(paths.pid_file(kind.name()))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn write_pid(paths: &InariPaths, kind: &ServiceKind, pid: u32) -> Result<()> {
    std::fs::create_dir_all(&paths.data)?;
    std::fs::write(paths.pid_file(kind.name()), pid.to_string())
        .map_err(|e| anyhow::anyhow!("Cannot write PID file: {e}"))
}

fn remove_pid(paths: &InariPaths, kind: &ServiceKind) {
    let _ = std::fs::remove_file(paths.pid_file(kind.name()));
}
