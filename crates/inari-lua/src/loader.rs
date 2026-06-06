use anyhow::{Context, Result};
use inari_core::config::{HookConfig, InariConfig, PortConfig, SiteConfig};
use inari_core::paths::InariPaths;
use inari_core::settings::Settings;
use mlua::prelude::*;
use std::path::Path;
use tracing::debug;

/// Load an Inari flavor from a Lua file and return the parsed config.
pub fn load_flavor(path: &Path) -> Result<InariConfig> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read flavor file: {:?}", path))?;

    let lua = Lua::new();
    lua.load(&source)
        .exec()
        .map_err(|e| anyhow::anyhow!("Lua error in flavor {:?}: {}", path, e))?;

    let globals = lua.globals();

    // flavor name
    let flavor: String = globals
        .get::<String>("flavor")
        .unwrap_or_else(|_| "default".to_string());

    // ports table
    let ports = globals
        .get::<LuaTable>("ports")
        .map(|t| PortConfig {
            panel: t.get::<u16>("panel").unwrap_or(1788),
            web:   t.get::<u16>("web").unwrap_or(8080),
            mysql: t.get::<u16>("mysql").unwrap_or(3307),
            redis: t.get::<u16>("redis").unwrap_or(6380),
        })
        .unwrap_or_default();

    // sites array
    let sites = globals
        .get::<LuaTable>("sites")
        .map(|t| {
            t.sequence_values::<LuaTable>()
                .filter_map(|r| r.ok())
                .map(|site| SiteConfig {
                    name:  site.get::<String>("name").unwrap_or_else(|_| "unnamed".to_string()),
                    root:  site.get::<String>("root").unwrap_or_else(|_| "sites/default".to_string()),
                    index: site.get::<String>("index").ok(),
                })
                .collect()
        })
        .unwrap_or_else(|_| vec![SiteConfig {
            name:  "default".to_string(),
            root:  "sites/default".to_string(),
            index: Some("index.php".to_string()),
        }]);

    // optional nginx template string
    let nginx_template: Option<String> = globals.get::<String>("nginx_template").ok();

    // optional root password for graceful mysqladmin shutdown
    let mysql_password: Option<String> = globals
        .get::<String>("mysql_password")
        .ok()
        .filter(|s| !s.is_empty());

    // hooks
    let hooks = globals
        .get::<LuaTable>("hooks")
        .map(|t| HookConfig {
            on_start: read_string_seq(&t, "on_start"),
            on_stop:  read_string_seq(&t, "on_stop"),
        })
        .unwrap_or_default();

    debug!("Loaded flavor '{}' with {} site(s)", flavor, sites.len());

    Ok(InariConfig { flavor, ports, sites, nginx_template, hooks, mysql_password })
}

/// Load the active flavor named in settings.json (`flavors/<name>.lua`), falling
/// back to `default.lua` and then to the built-in default config.
pub fn load_active_flavor(paths: &InariPaths) -> InariConfig {
    let name = Settings::load(&paths.data)
        .flavor
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "default".to_string());
    let candidate = paths.flavor(&name);
    let path = if candidate.exists() {
        candidate
    } else {
        paths.default_flavor()
    };
    if path.exists() {
        match load_flavor(&path) {
            Ok(cfg) => return cfg,
            Err(e) => tracing::warn!("Failed to load flavor {:?}: {e}", path),
        }
    }
    InariConfig::default()
}

fn read_string_seq(table: &LuaTable, key: &str) -> Vec<String> {
    table
        .get::<LuaTable>(key)
        .map(|t| t.sequence_values::<String>().filter_map(|v| v.ok()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_flavor_parses_ports_and_sites() {
        let lua = r#"
flavor = "test"
ports  = { panel = 1788, web = 8080, mysql = 3307, redis = 6380 }
sites  = { { name = "default", root = "sites/default", index = "index.php" } }
hooks  = { on_start = {}, on_stop = {} }
"#;
        let path = std::env::temp_dir().join("inari_test_flavor.lua");
        std::fs::write(&path, lua).unwrap();
        let cfg = load_flavor(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(cfg.flavor, "test");
        assert_eq!(cfg.ports.panel, 1788);
        assert_eq!(cfg.ports.web,   8080);
        assert_eq!(cfg.ports.mysql, 3307);
        assert_eq!(cfg.ports.redis, 6380);
        assert_eq!(cfg.sites.len(), 1);
        assert_eq!(cfg.sites[0].name, "default");
        assert_eq!(cfg.sites[0].index.as_deref(), Some("index.php"));
    }
}
