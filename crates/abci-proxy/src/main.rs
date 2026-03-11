mod codec;
mod translator;

use std::path::PathBuf;

use clap::Parser;
use eyre::{Result, WrapErr};
use futures::{SinkExt, StreamExt};
use prost::Message;
use tendermint_proto::v0_38::abci as pb;
use tokio::net::{UnixListener, UnixStream};
use tokio::signal;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tinycomet_types::*;

use crate::codec::VarintCodec;
use crate::translator::{abci_to_app_request, app_response_to_abci, proxy_response};

#[derive(Parser)]
#[command(name = "tinycomet-proxy")]
struct Cli {
    #[arg(long, default_value = "/tmp/app.sock")]
    app_socket: PathBuf,
    #[arg(long, default_value = "/tmp/cmt.sock")]
    cmt_socket: PathBuf,
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
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    if cli.cmt_socket.exists() {
        std::fs::remove_file(&cli.cmt_socket).wrap_err("failed to remove stale cmt socket")?;
    }

    if !cli.app_socket.exists() {
        eyre::bail!(
            "app socket {} does not exist — start tinycomet-app first",
            cli.app_socket.display()
        );
    }

    let listener = UnixListener::bind(&cli.cmt_socket)
        .wrap_err_with(|| format!("failed to bind {}", cli.cmt_socket.display()))?;

    tracing::info!(
        "proxy listening on {} -> forwarding to {}",
        cli.cmt_socket.display(),
        cli.app_socket.display()
    );

    let cmt_socket_path = cli.cmt_socket.clone();
    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        tracing::info!("shutting down");
        let _ = std::fs::remove_file(&cmt_socket_path);
        std::process::exit(0);
    });

    loop {
        let (cmt_stream, _) = listener.accept().await?;
        let app_socket = cli.app_socket.clone();
        tokio::spawn(async move {
            let app_stream = match UnixStream::connect(&app_socket).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("failed to connect to app-shell: {e}");
                    return;
                }
            };
            if let Err(e) = handle_cmt_connection(cmt_stream, app_stream).await {
                tracing::error!("connection error: {e:#}");
            }
        });
    }
}

async fn handle_cmt_connection(cmt_stream: UnixStream, app_stream: UnixStream) -> Result<()> {
    tracing::debug!("new CometBFT connection");
    let mut cmt = Framed::new(cmt_stream, VarintCodec);
    let mut app = Framed::new(app_stream, ipc_codec());

    while let Some(frame) = cmt.next().await {
        let request_bytes = frame.wrap_err("failed to read CometBFT frame")?;
        let request =
            pb::Request::decode(&*request_bytes).wrap_err("failed to decode ABCI request")?;

        let request_value = match request.value {
            Some(v) => v,
            None => {
                tracing::warn!("empty ABCI request");
                continue;
            }
        };

        let response_value = if let Some(resp) = proxy_response(&request_value) {
            resp
        } else if let Some(app_request) = abci_to_app_request(&request_value) {
            let app_response = forward_to_app(&mut app, &app_request).await?;
            app_response_to_abci(app_response)
        } else {
            tracing::warn!("unhandled ABCI request type");
            continue;
        };

        let response = pb::Response {
            value: Some(response_value),
        };
        cmt.send(response.encode_to_vec().into())
            .await
            .wrap_err("failed to send ABCI response")?;
    }

    tracing::debug!("CometBFT connection closed");
    Ok(())
}

async fn forward_to_app(
    app: &mut Framed<UnixStream, LengthDelimitedCodec>,
    request: &AppRequest,
) -> Result<AppResponse> {
    let request_bytes = borsh::to_vec(request)?;
    app.send(request_bytes.into())
        .await
        .wrap_err("failed to send to app")?;

    let buf = app
        .next()
        .await
        .ok_or_else(|| eyre::eyre!("app connection closed"))?
        .wrap_err("failed to read app response")?;

    borsh::from_slice(&buf).wrap_err("failed to deserialize AppResponse")
}
