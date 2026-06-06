//! GUI-editable settings layer.
//!
//! `flavor.lua` provides the defaults (and is hand-editable). The GUI writes a
//! `data/settings.json` overlay that wins over the flavor. This keeps the GUI
//! as the primary config surface without clobbering the user's Lua file.
//!
//! Apply model: ports/sites changes are written immediately, but take effect
//! when the affected service is (re)started — nginx.conf is regenerated on
//! every start, so a restart is enough; no app relaunch needed.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{InariConfig, SiteConfig};

/// Persisted, GUI-editable overrides. All fields optional → absent = use flavor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Active flavor name (loads flavors/<name>.lua). None/absent = "default".
    #[serde(default)]
    pub flavor: Option<String>,
    #[serde(default)]
    pub ports: PortOverrides,
    /// If Some, replaces the flavor site list entirely.
    #[serde(default)]
    pub sites: Option<Vec<SiteOverride>>,
    /// Services to auto-start when the app launches (by kind name).
    /// None = don't auto-start anything; Some([...]) = start these.
    #[serde(default)]
    pub autostart: Option<Vec<String>>,
    /// Launch Inari automatically when Windows starts (per-user).
    /// This only opens Inari; which services then start is governed by
    /// `autostart`. The two are intentionally independent.
    #[serde(default)]
    pub run_at_startup: bool,
    /// Start hidden in the tray instead of showing the main window on launch.
    /// Independent of `run_at_startup` and `autostart`.
    #[serde(default)]
    pub start_minimized: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortOverrides {
    pub panel: Option<u16>,
    pub web:   Option<u16>,
    pub mysql: Option<u16>,
    pub redis: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteOverride {
    pub name:  String,
    pub root:  String,
    #[serde(default)]
    pub index: Option<String>,
}

impl Settings {
    /// Load settings.json from the data dir. Missing/invalid → defaults.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("settings.json");
        match std::fs::read_to_string(&path) {
            // Strip a UTF-8 BOM if present — editors/PowerShell often add one,
            // and serde_json refuses to parse it, which would silently wipe
            // user settings (e.g. autostart) back to defaults.
            Ok(s) => {
                let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
                serde_json::from_str(s).unwrap_or_else(|e| {
                    tracing::warn!("Invalid settings.json ({e}); using defaults");
                    Settings::default()
                })
            }
            Err(_) => Settings::default(),
        }
    }

    /// Persist settings.json (pretty-printed) into the data dir.
    pub fn save(&self, data_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("settings.json");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Apply these overrides onto a base config (from flavor.lua).
    pub fn apply_to(&self, mut config: InariConfig) -> InariConfig {
        if let Some(p) = self.ports.panel { config.ports.panel = p; }
        if let Some(p) = self.ports.web   { config.ports.web   = p; }
        if let Some(p) = self.ports.mysql { config.ports.mysql = p; }
        if let Some(p) = self.ports.redis { config.ports.redis = p; }

        if let Some(sites) = &self.sites {
            if !sites.is_empty() {
                config.sites = sites
                    .iter()
                    .map(|s| SiteConfig {
                        name:  s.name.clone(),
                        root:  s.root.clone(),
                        index: s.index.clone().or_else(|| Some("index.php".to_string())),
                    })
                    .collect();
            }
        }
        config
    }
}
