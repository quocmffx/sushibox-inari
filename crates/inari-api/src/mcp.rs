//! Minimal MCP (Model Context Protocol) server over HTTP / JSON-RPC 2.0.
//!
//! Exposes Inari dev-stack control as MCP tools so an AI agent (Claude Code,
//! etc.) can drive the stack. Runs as a second axum server on its own port,
//! toggled on/off from the panel (`POST /api/mcp/start | /api/mcp/stop`) and
//! sharing the panel's `Arc<AppState>`.
//!
//! Speaks: initialize / notifications/initialized / ping / tools/list /
//! tools/call. Tool calls return the standard `{ content: [...], isError }`.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tower_http::cors::CorsLayer;

use crate::AppState;
use inari_core::process::{ServiceKind, ServiceStatus};

/// Loopback port the MCP server binds to when toggled on.
pub const MCP_PORT: u16 = 1789;
/// MCP protocol revision we implement (widely supported by current clients).
const PROTOCOL_VERSION: &str = "2024-11-05";

/// MCP endpoint URL an AI client should connect to.
pub fn mcp_url() -> String {
    format!("http://127.0.0.1:{MCP_PORT}/mcp")
}

pub fn is_running(state: &AppState) -> bool {
    state.mcp.lock().unwrap().is_some()
}

/// Stop the MCP server (graceful shutdown). Returns true if one was running.
pub fn stop(state: &AppState) -> bool {
    if let Some(tx) = state.mcp.lock().unwrap().take() {
        let _ = tx.send(());
        true
    } else {
        false
    }
}

/// Start the MCP server on MCP_PORT. Errors if already running or the port is taken.
pub async fn start(state: Arc<AppState>) -> Result<u16, String> {
    if is_running(&state) {
        return Err("MCP server đã chạy".to_string());
    }
    let addr = format!("127.0.0.1:{MCP_PORT}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("không bind được {addr}: {e}"))?;

    let app = Router::new()
        .route("/mcp", post(handle))
        .route("/", post(handle))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let (tx, rx) = oneshot::channel::<()>();
    *state.mcp.lock().unwrap() = Some(tx);

    let on_exit = state.clone();
    tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = rx.await;
        });
        if let Err(e) = server.await {
            tracing::error!("MCP server error: {e}");
        }
        *on_exit.mcp.lock().unwrap() = None;
    });

    Ok(MCP_PORT)
}

// ---------------------------------------------------------------------------
// JSON-RPC dispatch
// ---------------------------------------------------------------------------

async fn handle(State(state): State<Arc<AppState>>, Json(req): Json<Value>) -> Response {
    match dispatch(&state, req).await {
        Some(resp) => Json(resp).into_response(),
        None => StatusCode::ACCEPTED.into_response(), // notification: no body
    }
}

async fn dispatch(state: &Arc<AppState>, req: Value) -> Option<Value> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");

    match method {
        "initialize" => Some(ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "inari", "version": env!("CARGO_PKG_VERSION") }
            }),
        )),
        // Notifications carry no id and expect no response.
        m if m.starts_with("notifications/") => None,
        "ping" => Some(ok(id, json!({}))),
        "tools/list" => Some(ok(id, json!({ "tools": tool_defs() }))),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let st = state.clone();
            let res = tokio::task::spawn_blocking(move || call_tool(&st, &name, &args))
                .await
                .unwrap_or_else(|_| Err("internal task error".to_string()));
            Some(match res {
                Ok(text) => ok(id, json!({ "content": [text_content(text)], "isError": false })),
                Err(e) => ok(id, json!({ "content": [text_content(e)], "isError": true })),
            })
        }
        _ => Some(err(id, -32601, "Method not found")),
    }
}

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
fn text_content(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

fn tool_defs() -> Value {
    let svc_arg = json!({
        "type": "object",
        "properties": {
            "service": {
                "type": "string",
                "enum": ["all", "nginx", "php", "mysql", "redis"],
                "description": "Service to act on, or 'all' for the whole stack."
            }
        },
        "required": ["service"]
    });
    json!([
        {
            "name": "inari_status",
            "description": "Status of all Inari dev services (nginx/php/mysql/redis): running state, pid, port.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "inari_start",
            "description": "Start a service or 'all'. nginx auto-starts its PHP dependency.",
            "inputSchema": svc_arg
        },
        {
            "name": "inari_stop",
            "description": "Stop a service or 'all'.",
            "inputSchema": svc_arg
        },
        {
            "name": "inari_restart",
            "description": "Restart a service or 'all'.",
            "inputSchema": svc_arg
        }
    ])
}

fn call_tool(state: &AppState, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "inari_status" => Ok(status_text(state)),
        "inari_start" => act(state, args, Act::Start),
        "inari_stop" => act(state, args, Act::Stop),
        "inari_restart" => act(state, args, Act::Restart),
        other => Err(format!("unknown tool: {other}")),
    }
}

#[derive(Clone, Copy)]
enum Act {
    Start,
    Stop,
    Restart,
}

fn act(state: &AppState, args: &Value, action: Act) -> Result<String, String> {
    let svc = args
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    if svc.is_empty() {
        return Err("thiếu tham số 'service'".to_string());
    }

    let targets: Vec<ServiceKind> = if svc == "all" {
        let order: Vec<ServiceKind> = match action {
            Act::Stop => ServiceKind::stop_order().to_vec(),
            _ => ServiceKind::start_order().to_vec(),
        };
        let public = ServiceKind::public().to_vec();
        order.into_iter().filter(|k| public.contains(k)).collect()
    } else {
        match crate::parse_kind(&svc) {
            Some(k) => vec![k],
            None => return Err(format!("service không hợp lệ: {svc}")),
        }
    };

    let mut lines = Vec::new();
    for kind in targets {
        let line = match action {
            Act::Start => {
                let (ok, msg) = crate::start_public_service_inner(&kind, state);
                format!(
                    "{}: {}",
                    kind.display_name(),
                    if ok { msg } else { format!("FAIL {msg}") }
                )
            }
            Act::Stop => match crate::stop_public_service_inner(&kind, state) {
                Ok(()) => format!("{}: stopped", kind.display_name()),
                Err(e) => format!("{}: {e}", kind.display_name()),
            },
            Act::Restart => {
                let _ = crate::stop_public_service_inner(&kind, state);
                let (ok, msg) = crate::start_public_service_inner(&kind, state);
                format!(
                    "{}: {}",
                    kind.display_name(),
                    if ok { msg } else { format!("FAIL {msg}") }
                )
            }
        };
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

fn status_text(state: &AppState) -> String {
    let statuses = inari_core::state::statuses(&state.paths, ServiceKind::public(), |_| true);
    let cfg = state.config.lock().unwrap().clone();
    let arr: Vec<Value> = statuses
        .iter()
        .map(|(kind, status)| {
            let (running, pid) = match status {
                ServiceStatus::Running { pid } => (true, Some(*pid)),
                _ => (false, None),
            };
            json!({
                "service": kind.name(),
                "name": kind.display_name(),
                "running": running,
                "pid": pid,
                "port": crate::service_port(kind, &cfg),
            })
        })
        .collect();
    serde_json::to_string_pretty(&json!({ "services": arr })).unwrap_or_else(|_| "{}".to_string())
}
