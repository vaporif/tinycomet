mod app;
mod state;
mod storage;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use eyre::{Result, WrapErr};
use futures::{SinkExt, StreamExt};
use tinycomet_types::*;
use tokio::net::UnixListener;
use tokio::signal;
use tokio::sync::RwLock;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::state::State;
use crate::storage::Storage;

#[derive(Parser)]
#[command(name = "tinycomet-app")]
struct Cli {
    #[arg(long, default_value = "/tmp/app.sock")]
    socket: PathBuf,
    #[arg(long, default_value = "./data/tinycomet.db")]
    db_path: PathBuf,
}

fn ipc_codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .length_field_type::<u32>()
        .little_endian()
        .max_frame_length(MAX_FRAME_SIZE as usize)
        .new_codec()
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    if cli.socket.exists() {
        std::fs::remove_file(&cli.socket).wrap_err("failed to remove stale socket")?;
    }

    let storage = Storage::open(&cli.db_path)?;
    let state = State::new(storage)?;
    let state = Arc::new(RwLock::new(state));

    let listener = UnixListener::bind(&cli.socket)
        .wrap_err_with(|| format!("failed to bind {}", cli.socket.display()))?;
    tracing::info!("app-shell listening on {}", cli.socket.display());

    let socket_path = cli.socket.clone();
    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        tracing::info!("shutting down");
        let _ = std::fs::remove_file(&socket_path);
        std::process::exit(0);
    });

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                tracing::error!("connection error: {e:#}");
            }
        });
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: Arc<RwLock<State>>,
) -> Result<()> {
    tracing::debug!("new connection accepted");
    let mut framed = Framed::new(stream, ipc_codec());

    while let Some(frame) = framed.next().await {
        let buf = frame.wrap_err("failed to read frame")?;
        let request: AppRequest =
            borsh::from_slice(&buf).wrap_err("failed to deserialize AppRequest")?;

        let response = dispatch_request(request, &state).await;
        let response_bytes = borsh::to_vec(&response)?;
        framed
            .send(response_bytes.into())
            .await
            .wrap_err("failed to send response")?;
    }

    tracing::debug!("connection closed");
    Ok(())
}

async fn dispatch_request(request: AppRequest, state: &Arc<RwLock<State>>) -> AppResponse {
    match request {
        AppRequest::Info => {
            let state = state.read().await;
            state.handle_info()
        }
        AppRequest::InitChain {
            chain_id,
            initial_height: _,
            app_state,
        } => {
            let mut state = state.write().await;
            state.handle_init_chain(chain_id, &app_state)
        }
        AppRequest::CheckTx { tx_bytes } => {
            let state = state.read().await;
            state.handle_check_tx(&tx_bytes)
        }
        AppRequest::PrepareProposal { txs, max_tx_bytes } => {
            let state = state.read().await;
            state.handle_prepare_proposal(txs, max_tx_bytes)
        }
        AppRequest::ProcessProposal { txs } => {
            let state = state.read().await;
            state.handle_process_proposal(&txs)
        }
        AppRequest::FinalizeBlock { txs, height, time } => {
            let mut state = state.write().await;
            state.handle_finalize_block(txs, height, time)
        }
        AppRequest::Commit => {
            let mut state = state.write().await;
            state.handle_commit()
        }
        AppRequest::Query { path, data } => {
            let state = state.read().await;
            state.handle_query(&path, &data)
        }
    }
}
