use anyhow::Result;
use inari_core::paths::InariPaths;

use crate::supervisor::load_config;

pub async fn run() -> Result<()> {
    let paths  = InariPaths::from_exe()?;
    let config = load_config(&paths);
    let port   = config.ports.panel;
    let url    = format!("http://127.0.0.1:{port}");

    println!("Starting panel on {url}");

    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn();
    }

    inari_api::start_server(port, paths, config).await
}
