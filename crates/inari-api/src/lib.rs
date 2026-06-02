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
    process::{ServiceKind, ServiceStatus, spawn_service},
    runtime::{descriptor_for, generate_nginx_conf, generate_php_ini, init_mysql_if_needed, stop_service, wait_for_exit},
    settings::Settings,
    state,
};
use mime_guess::from_path;
use rust_embed::RustEmbed;
use serde_json::json;
use tower_http::cors::CorsLayer;

/// Grace period after spawn before trusting a service as "up" (see start.rs).
const LIVENESS_GRACE_MS: u64 = 400;

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
    // Already running? (PID-reuse-aware; cleans up a stale PID file itself.)
    if state::is_running(paths, kind) {
        let pid = read_pid(paths, kind).unwrap_or(0);
        return (false, format!("already running (pid {pid})"));
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
            // Confirm the process didn't exit immediately (port clash, missing
            // DLL, bad datadir) before reporting success and persisting a PID.
            if !state::confirm_alive(pid, kind, LIVENESS_GRACE_MS) {
                // Try to diagnose which port is in use for a better error.
                let port = service_port(kind, config);
                let conflict = inari_core::process::port_in_use(port)
                    .map(|(p, n)| format!(" (in use by pid {} {})", p, n))
                    .unwrap_or_default();
                return (
                    false,
                    format!("{} exited immediately — port {} already in use{}?", kind.display_name(), port, conflict),
                );
            }
            match write_pid(paths, kind, pid) {
                Ok(_)  => (true, format!("{} started (pid {pid})", kind.display_name())),
                Err(e) => (false, e.to_string()),
            }
        }
        Err(e) => (false, e.to_string()),
    }
}

fn stop_service_inner(kind: &ServiceKind, state: &AppState) -> Result<(), String> {
    let Some(pid) = read_pid(&state.paths, kind) else {
        return Err("not running".to_string());
    };
    // Guard against PID reuse: only act if the recorded PID still belongs to
    // our service. If it's been recycled, just clear the stale file.
    if !state::is_running(&state.paths, kind) {
        remove_pid(&state.paths, kind);
        return Err("not running".to_string());
    }
    let port = service_port(kind, &state.config.lock().unwrap());
    stop_service(&state.paths, kind, port);
    wait_for_exit(pid, 5000);
    remove_pid(&state.paths, kind);
    Ok(())
}

/// Public nginx controls own PHP-CGI as a private dependency.
fn start_public_service_inner(kind: &ServiceKind, state: &AppState) -> (bool, String) {
    if *kind == ServiceKind::Nginx && !state::is_running(&state.paths, &ServiceKind::Php) {
        let config = state.config.lock().unwrap().clone();
        let (ok, msg) = start_service_inner(&ServiceKind::Php, &state.paths, &config, &state.job);
        if !ok {
            return (false, format!("PHP dependency failed: {msg}"));
        }
    }

    let config = state.config.lock().unwrap().clone();
    start_service_inner(kind, &state.paths, &config, &state.job)
}

fn stop_public_service_inner(kind: &ServiceKind, state: &AppState) -> Result<(), String> {
    let result = stop_service_inner(kind, state);
    if *kind == ServiceKind::Nginx {
        let _ = stop_service_inner(&ServiceKind::Php, state);
    }
    result
}

/// Start services by name directly in-process (used by autostart at launch).
/// Skips unknown names silently. Runs synchronously — call from a bg thread.
///
/// Autostart does not run through the HTTP server state, so it needs its own
/// process-lifetime Job Object. We intentionally leak the handle after startup:
/// the OS closes it when Inari.exe exits or is killed, which triggers
/// KILL_ON_JOB_CLOSE and prevents service orphans.
pub fn start_services_direct(kinds: &[String], paths: &InariPaths, config: &InariConfig) {
    let job = JobObject::new().ok();

    // Reorder the requested set into dependency order (backends → PHP → nginx)
    // regardless of how they were listed in settings.json, so nginx never comes
    // up before its upstreams.
    let mut requested: Vec<ServiceKind> =
        kinds.iter().filter_map(|s| parse_kind(s)).collect();
    if requested.contains(&ServiceKind::Nginx) && !requested.contains(&ServiceKind::Php) {
        requested.push(ServiceKind::Php);
    }
    let unknown: Vec<&String> = kinds
        .iter()
        .filter(|s| parse_kind(s).is_none())
        .collect();
    for u in unknown {
        tracing::warn!("autostart: unknown service '{u}', skipping");
    }

    for kind in ServiceKind::start_order() {
        if !requested.contains(kind) {
            continue;
        }
        let (ok, msg) = start_service_inner(kind, paths, config, &job);
        if ok {
            tracing::info!("autostart: {msg}");
        } else {
            tracing::warn!("autostart: {} failed — {msg}", kind.name());
        }
    }

    if job.is_some() {
        std::mem::forget(job);
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
    // Run the blocking spawn + liveness check off the async workers so the
    // panel can keep serving status polls while a service is coming up.
    let st = state.clone();
    let (ok, msg) = tokio::task::spawn_blocking(move || {
        start_public_service_inner(&kind, &st)
    })
    .await
    .unwrap_or((false, "internal task error".to_string()));
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
    // Stop + wait-for-exit can block for seconds (MariaDB shutdown); keep it off
    // the async workers.
    let st = state.clone();
    let kind_task = kind.clone();
    let result = tokio::task::spawn_blocking(move || {
        stop_public_service_inner(&kind_task, &st)
    })
    .await
    .unwrap_or_else(|_| Err("internal task error".to_string()));
    match result {
        Ok(()) => {
            push_activity(&state, format!("{} stopped", kind.display_name()));
            axum::Json(json!({"ok": true}))
        }
        Err(e) => axum::Json(json!({"ok": false, "error": e})),
    }
}

async fn api_service_restart(
    Path(kind_str): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let kind = match parse_kind(&kind_str) {
        Some(k) => k,
        None => return axum::Json(json!({"ok": false, "error": "unknown service"})),
    };
    let st = state.clone();
    let kind_task = kind.clone();
    let (ok, msg) = tokio::task::spawn_blocking(move || {
        // Stop the current instance (if any) and wait for it to fully exit so
        // the port is released before we respawn — no fixed-sleep guesswork.
        // Only stop a PID that still belongs to our service (PID-reuse safe).
        let _ = stop_public_service_inner(&kind_task, &st);
        // Reuse the shared start path: conf generation, spawn, Job assignment
        // and liveness confirmation all happen identically to a normal start.
        start_public_service_inner(&kind_task, &st)
    })
    .await
    .unwrap_or((false, "internal task error".to_string()));
    if ok {
        push_activity(&state, format!("{} restarted", kind.display_name()));
        axum::Json(json!({"ok": true, "pid": msg}))
    } else {
        axum::Json(json!({"ok": false, "error": msg}))
    }
}

// ---------------------------------------------------------------------------
// API — status + config
// ---------------------------------------------------------------------------

async fn api_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Single source of truth: PID-reuse-aware liveness that also cleans up
    // stale PID files, so the panel can never disagree with the CLI.
    let statuses = state::statuses(&state.paths, ServiceKind::public(), |_| true);

    let config = state.config.lock().unwrap().clone();
    let services: Vec<_> = statuses
        .iter()
        .map(|(kind, status)| {
            let (running, pid) = match status {
                ServiceStatus::Running { pid } => (true, Some(*pid)),
                _ => (false, None),
            };
            json!({
                "kind":    kind.name(),
                "name":    kind.display_name(),
                "version": kind.version(),
                "state":   if running { "running" } else { "stopped" },
                "pid":     pid,
                "port":    service_port(kind, &config),
            })
        })
        .collect();

    axum::Json(json!({ "services": services }))
}

async fn api_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let config = state.config.lock().unwrap();
    axum::Json(config_json(&config))
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
    // Re-apply onto base flavor config → update live effective config.
    let effective = settings.apply_to(state.base.clone());
    {
        let mut cfg = state.config.lock().unwrap();
        *cfg = effective;
    }
    // Regenerate nginx.conf so port/site changes are written to disk even if
    // nginx is stopped. If nginx is running, a restart is still required.
    let nginx_running = state::is_running(&state.paths, &ServiceKind::Nginx);
    let nconf_ok = {
        let cfg = state.config.lock().unwrap();
        generate_nginx_conf(&state.paths, &cfg).is_ok()
    };
    let note = if !nconf_ok {
        "settings saved but nginx.conf write failed; check logs.".to_string()
    } else if nginx_running {
        "settings saved. Restart Nginx to apply port/site changes.".to_string()
    } else {
        "settings saved. Start Nginx to apply port/site changes.".to_string()
    };
    push_activity(&state, format!("settings updated — {note}"));
    // Reconcile the Windows "run at startup" registry entry with the setting.
    if let Err(e) = inari_core::startup::apply(settings.run_at_startup) {
        tracing::warn!("run_at_startup apply failed: {e}");
    }
    let saved = Settings::load(&state.paths.data);
    let config = state.config.lock().unwrap();
    axum::Json(json!({
        "ok": true,
        "note": note,
        "settings": saved,
        "config": config_json(&config),
    }))
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
    // URL targets open in the system browser; the rest open folders/files.
    if target == "web" || target == "repo" || target == "adminer" {
        let url = if target == "repo" {
            "https://github.com/quocmffx/sushibox-inari".to_string()
        } else if target == "adminer" {
            let config = state.config.lock().unwrap();
            format!("http://localhost:{}/_inari/adminer.php", config.ports.web)
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
        "phpIni" => state.paths.php_ini(),
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

fn config_json(config: &InariConfig) -> serde_json::Value {
    json!({
        "flavor": config.flavor,
        "ports": {
            "panel": config.ports.panel,
            "web":   config.ports.web,
            "mysql": config.ports.mysql,
            "redis": config.ports.redis,
        },
        "php": {
            "version": ServiceKind::Php.version(),
            "cgi_port": 9000,
        },
        "sites": config.sites.iter().map(|s| json!({
            "name": s.name,
            "root": s.root,
        })).collect::<Vec<_>>(),
    })
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
    state::read_pid(paths, kind)
}

fn write_pid(paths: &InariPaths, kind: &ServiceKind, pid: u32) -> Result<()> {
    state::write_pid(paths, kind, pid)
}

fn remove_pid(paths: &InariPaths, kind: &ServiceKind) {
    state::remove_pid(paths, kind)
}
