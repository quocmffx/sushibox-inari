use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InariConfig {
    pub flavor:         String,
    pub ports:          PortConfig,
    pub sites:          Vec<SiteConfig>,
    pub nginx_template: Option<String>,
    pub hooks:          HookConfig,
    /// Root password for graceful `mysqladmin shutdown`. None = passwordless root.
    /// Set by flavors whose root has a password (e.g. the KTM SDK uses 123456),
    /// otherwise stop would kill mysqld and force crash recovery.
    #[serde(default)]
    pub mysql_password: Option<String>,
}

impl Default for InariConfig {
    fn default() -> Self {
        Self {
            flavor: "default".to_string(),
            ports:  PortConfig::default(),
            sites:  vec![SiteConfig {
                name:  "default".to_string(),
                root:  "sites/default".to_string(),
                index: Some("index.php".to_string()),
            }],
            nginx_template: None,
            hooks: HookConfig::default(),
            mysql_password: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortConfig {
    pub panel: u16,
    pub web:   u16,
    pub mysql: u16,
    pub redis: u16,
}

impl Default for PortConfig {
    fn default() -> Self {
        Self {
            panel: 1788,
            web:   8080,
            mysql: 3307,
            redis: 6380,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteConfig {
    pub name:  String,
    pub root:  String,
    pub index: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookConfig {
    pub on_start: Vec<String>,
    pub on_stop:  Vec<String>,
}
