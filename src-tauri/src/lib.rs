use inari_core::{config::InariConfig, paths::InariPaths};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};

fn load_config() -> (InariPaths, InariConfig) {
    let paths = InariPaths::from_exe().expect("Cannot resolve exe path");
    let config = if paths.default_flavor().exists() {
        inari_lua::load_flavor(&paths.default_flavor()).unwrap_or_default()
    } else {
        InariConfig::default()
    };
    (paths, config)
}

fn start_panel(paths: InariPaths, config: InariConfig, port: u16) {
    std::thread::spawn(move || {
        // Multi-threaded (small pool) on purpose: the control handlers do
        // blocking work — spawning service processes, waiting for a clean exit,
        // MariaDB datadir init, post-spawn liveness checks. On a current-thread
        // runtime any one of those would freeze the whole panel (status polls
        // included) until it finished. A couple of workers keep the UI
        // responsive while a start/stop/restart is in flight.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("panel runtime");
        if let Err(e) = rt.block_on(inari_api::start_server(port, paths, config)) {
            eprintln!("Panel server exited: {e}");
        }
    });
}

/// Auto-start services listed in settings.json directly in-process.
/// No HTTP loopback — calls the same start logic the API handler uses.
fn run_autostart(paths: InariPaths, config: InariConfig) {
    use inari_core::settings::Settings;
    let settings = Settings::load(&paths.data);
    // Clone autostart list before consuming settings in apply_to.
    let kinds = match settings.autostart.clone() {
        Some(k) if !k.is_empty() => k,
        _ => return,
    };
    // Apply settings overlay so ports etc. are correct.
    let effective = settings.apply_to(config);
    std::thread::spawn(move || {
        inari_api::start_services_direct(&kinds, &paths, &effective);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (paths, config) = load_config();
    let port = config.ports.panel;
    // Read the "start minimized to tray" preference before building the window.
    let start_minimized = inari_core::settings::Settings::load(&paths.data).start_minimized;
    // Data dir for the portable window-position file (kept next to the exe, not
    // in %APPDATA%, so the install stays self-contained).
    let data_dir = paths.data.clone();
    let data_dir_setup = data_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // A second instance tried to launch — focus the existing window instead.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(move |app| {
            // Start the panel server here (only the primary instance reaches
            // setup; the single-instance plugin redirects any 2nd launch above).
            start_panel(paths.clone(), config.clone(), port);
            // Auto-start configured services (from settings.json) directly in-process.
            run_autostart(paths, config);

            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu   = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

            // show_menu_on_left_click(false): left = toggle window, right = menu (Tauri default).
            let mut tray = TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Inari — SushiBox");
            // Use the app's bundled icon for the tray (not set automatically).
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            let win = app.get_webview_window("main").expect("main window");
            win.eval(&format!("window.location.replace('http://127.0.0.1:{port}/')"))?;
            // Restore the last position the user left the window at; on first run
            // (no saved position) dock to the bottom-right of the primary monitor
            // above the taskbar, like PowerToys / PC Manager.
            if !restore_window_pos(&win, &data_dir_setup) {
                position_bottom_right(&win, 16);
            }
            // The window starts hidden (visible:false in config). Show it unless
            // the user chose to start minimized to tray. This is independent of
            // run-at-startup and per-service auto-start.
            if !start_minimized {
                let _ = win.show();
                let _ = win.set_focus();
            }
            Ok(())
        })
        .on_tray_icon_event(|app, event| {
            // Left click only — right-click menu is handled automatically by Tauri.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event {
                toggle_window(app);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_window(app),
            "hide" => { if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); } }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("tauri build failed")
        .run(move |app, event| {
            if let RunEvent::WindowEvent {
                label,
                event: win_event,
                ..
            } = &event {
                if label == "main" {
                    match win_event {
                        WindowEvent::CloseRequested { api, .. } => {
                            api.prevent_close();
                            if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); }
                        }
                        // Remember where the user drags the window to.
                        WindowEvent::Moved(_) => {
                            if let Some(w) = app.get_webview_window("main") {
                                save_window_pos(&w, &data_dir);
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
}

fn toggle_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_window(app);
        }
    }
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Dock the window to the bottom-right of the primary monitor's work area,
/// leaving `margin` logical px from the right and bottom edges. Uses the
/// monitor work area (excludes the taskbar) so the window sits just above it.
fn position_bottom_right(win: &tauri::WebviewWindow, margin: i32) {
    use tauri::{PhysicalPosition, Position};
    let monitor = match win.primary_monitor() {
        Ok(Some(m)) => m,
        _ => match win.current_monitor() {
            Ok(Some(m)) => m,
            _ => return,
        },
    };
    let scale = monitor.scale_factor();
    let mon_pos = monitor.position();   // physical, top-left of the monitor
    let mon_size = monitor.size();      // physical, full monitor size
    let win_size = match win.outer_size() {
        Ok(s) => s,
        Err(_) => return,
    };

    // Approximate the work area: full monitor minus a typical taskbar band at
    // the bottom. Tauri 2 doesn't expose work area directly, so reserve the
    // taskbar height (48 logical px is the Win11 default) plus the margin.
    let taskbar_px = (48.0 * scale) as i32;
    let margin_px = (margin as f64 * scale) as i32;

    let x = mon_pos.x + mon_size.width as i32 - win_size.width as i32 - margin_px;
    let y = mon_pos.y + mon_size.height as i32 - win_size.height as i32 - taskbar_px - margin_px;

    let _ = win.set_position(Position::Physical(PhysicalPosition { x, y }));
}

/// Persist the window's physical position to `data/window.json` (portable —
/// next to the exe, not %APPDATA%). Best-effort; ignores errors.
fn save_window_pos(win: &tauri::WebviewWindow, data_dir: &std::path::Path) {
    if let Ok(pos) = win.outer_position() {
        let _ = std::fs::create_dir_all(data_dir);
        let json = serde_json::json!({ "x": pos.x, "y": pos.y });
        let _ = std::fs::write(data_dir.join("window.json"), json.to_string());
    }
}

/// Restore the saved window position. Returns true if a valid saved position was
/// applied; false if there was none (caller then falls back to docking).
fn restore_window_pos(win: &tauri::WebviewWindow, data_dir: &std::path::Path) -> bool {
    use tauri::{PhysicalPosition, Position};
    let path = data_dir.join("window.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    let (Some(x), Some(y)) = (v.get("x").and_then(|n| n.as_i64()),
                              v.get("y").and_then(|n| n.as_i64())) else { return false };
    // Guard against off-screen positions from a disconnected monitor: only
    // accept if the point falls inside some currently-available monitor.
    let on_screen = win.available_monitors().map(|mons| {
        mons.iter().any(|m| {
            let p = m.position();
            let s = m.size();
            x >= p.x as i64 && x < (p.x as i64 + s.width as i64)
                && y >= p.y as i64 && y < (p.y as i64 + s.height as i64)
        })
    }).unwrap_or(false);
    if !on_screen {
        return false;
    }
    win.set_position(Position::Physical(PhysicalPosition { x: x as i32, y: y as i32 })).is_ok()
}
