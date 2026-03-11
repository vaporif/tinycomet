use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use eyre::{Result, WrapErr};
use tokio::process::Command;
use tokio::signal;

#[derive(Parser)]
#[command(name = "tinycomet", about = "Launch the full tinycomet stack")]
struct Cli {
    #[arg(long, default_value = "/tmp/app.sock")]
    app_socket: PathBuf,

    #[arg(long, default_value = "/tmp/cmt.sock")]
    cmt_socket: PathBuf,

    #[arg(long, default_value = "./data/tinycomet.db")]
    db_path: PathBuf,

    #[arg(long, default_value_t = default_cometbft_home())]
    cometbft_home: String,

    #[arg(long)]
    rpc_laddr: Option<String>,
}

fn default_cometbft_home() -> String {
    dirs_home().unwrap_or_else(|| ".cometbft".to_string())
}

fn dirs_home() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|h| format!("{h}/.cometbft"))
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    if !PathBuf::from(&cli.cometbft_home).join("config").exists() {
        tracing::info!("initializing CometBFT at {}", cli.cometbft_home);
        let status = Command::new("cometbft")
            .args(["init", "--home", &cli.cometbft_home])
            .status()
            .await
            .wrap_err("failed to run cometbft init — is cometbft installed?")?;
        if !status.success() {
            eyre::bail!("cometbft init failed");
        }
    }

    let config_path = PathBuf::from(&cli.cometbft_home).join("config/config.toml");
    let config = std::fs::read_to_string(&config_path)
        .wrap_err("failed to read CometBFT config")?;

    let proxy_line = format!("proxy_app = \"unix:///{}\"", cli.cmt_socket.display());
    let mut in_rpc_section = false;
    let mut patched = false;

    let updated: String = config
        .lines()
        .map(|line| {
            if line.starts_with("proxy_app") {
                patched = true;
                return proxy_line.clone();
            }
            if line.starts_with('[') {
                in_rpc_section = line.starts_with("[rpc]");
            }
            if in_rpc_section && line.starts_with("laddr") {
                if let Some(ref rpc_laddr) = cli.rpc_laddr {
                    patched = true;
                    return format!("laddr = \"tcp://{rpc_laddr}\"");
                }
            }
            if line.starts_with("db_backend") && !line.contains("goleveldb") {
                patched = true;
                return "db_backend = \"goleveldb\"".to_string();
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    if patched {
        std::fs::write(&config_path, &updated)
            .wrap_err("failed to write CometBFT config")?;
        tracing::info!("patched CometBFT config");
    }

    let bin_dir = std::env::current_exe()
        .wrap_err("failed to determine current executable path")?
        .parent()
        .expect("executable must have a parent directory")
        .to_path_buf();
    let app_bin = bin_dir.join("tinycomet-app");
    let proxy_bin = bin_dir.join("tinycomet-proxy");

    if !app_bin.exists() {
        eyre::bail!("tinycomet-app not found at {} — run `cargo build` first", app_bin.display());
    }
    if !proxy_bin.exists() {
        eyre::bail!("tinycomet-proxy not found at {} — run `cargo build` first", proxy_bin.display());
    }

    let _ = std::fs::remove_file(&cli.app_socket);
    let _ = std::fs::remove_file(&cli.cmt_socket);

    tracing::info!("starting tinycomet-app");
    let mut app = Command::new(&app_bin)
        .args([
            "--socket", &cli.app_socket.to_string_lossy(),
            "--db-path", &cli.db_path.to_string_lossy(),
        ])
        .kill_on_drop(true)
        .spawn()
        .wrap_err("failed to start tinycomet-app")?;

    wait_for_socket(&cli.app_socket, Duration::from_secs(10)).await
        .wrap_err("tinycomet-app did not create socket in time")?;
    tracing::info!("tinycomet-app ready");

    tracing::info!("starting tinycomet-proxy");
    let mut proxy = Command::new(&proxy_bin)
        .args([
            "--app-socket", &cli.app_socket.to_string_lossy(),
            "--cmt-socket", &cli.cmt_socket.to_string_lossy(),
        ])
        .kill_on_drop(true)
        .spawn()
        .wrap_err("failed to start tinycomet-proxy")?;

    wait_for_socket(&cli.cmt_socket, Duration::from_secs(10)).await
        .wrap_err("tinycomet-proxy did not create socket in time")?;
    tracing::info!("tinycomet-proxy ready");

    tracing::info!("starting cometbft");
    let mut cometbft = Command::new("cometbft")
        .args(["start", "--home", &cli.cometbft_home])
        .kill_on_drop(true)
        .spawn()
        .wrap_err("failed to start cometbft")?;

    tracing::info!("all processes running — press Ctrl+C to stop");

    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("shutting down all processes");
        }
        status = app.wait() => {
            tracing::error!("tinycomet-app exited: {status:?}");
        }
        status = proxy.wait() => {
            tracing::error!("tinycomet-proxy exited: {status:?}");
        }
        status = cometbft.wait() => {
            tracing::error!("cometbft exited: {status:?}");
        }
    }

    let _ = app.kill().await;
    let _ = proxy.kill().await;
    let _ = cometbft.kill().await;

    let _ = std::fs::remove_file(&cli.app_socket);
    let _ = std::fs::remove_file(&cli.cmt_socket);

    tracing::info!("shutdown complete");
    Ok(())
}

async fn wait_for_socket(path: &std::path::Path, timeout: Duration) -> Result<()> {
    let start = tokio::time::Instant::now();
    loop {
        if path.exists() {
            return Ok(());
        }
        if start.elapsed() > timeout {
            eyre::bail!("timeout waiting for socket {}", path.display());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
