mod translator;

use std::path::PathBuf;

use clap::Parser;
use eyre::{Result, WrapErr};
use prost::Message;
use tendermint_proto::v0_38::abci as pb;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal;
use tinycomet_types::*;

use crate::translator::{abci_to_app_request, app_response_to_abci, proxy_response};

#[derive(Parser)]
#[command(name = "tinycomet-proxy")]
struct Cli {
    #[arg(long, default_value = "/tmp/app.sock")]
    app_socket: PathBuf,
    #[arg(long, default_value = "/tmp/cmt.sock")]
    cmt_socket: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
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

async fn handle_cmt_connection(mut cmt: UnixStream, mut app: UnixStream) -> Result<()> {
    tracing::debug!("new CometBFT connection");
    loop {
        let request_bytes = match read_varint_prefixed(&mut cmt).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                tracing::debug!("CometBFT connection closed");
                return Ok(());
            }
            Err(e) => return Err(e),
        };

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
        let response_bytes = response.encode_to_vec();
        write_varint_prefixed(&mut cmt, &response_bytes).await?;
    }
}

async fn forward_to_app(app: &mut UnixStream, request: &AppRequest) -> Result<AppResponse> {
    let request_bytes = borsh::to_vec(request)?;
    app.write_u32_le(request_bytes.len() as u32).await?;
    app.write_all(&request_bytes).await?;
    app.flush().await?;

    let len = app.read_u32_le().await?;
    if len > MAX_FRAME_SIZE {
        eyre::bail!("app response frame too large: {len}");
    }
    let mut buf = vec![0u8; len as usize];
    app.read_exact(&mut buf).await?;
    borsh::from_slice(&buf).wrap_err("failed to deserialize AppResponse")
}

async fn read_varint_prefixed(stream: &mut UnixStream) -> Result<Option<Vec<u8>>> {
    let mut len: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = match stream.read_u8().await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        len |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            eyre::bail!("varint too long");
        }
    }
    let len = len as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(Some(buf))
}

async fn write_varint_prefixed(stream: &mut UnixStream, data: &[u8]) -> Result<()> {
    let mut len = data.len() as u64;
    let mut varint_buf = Vec::with_capacity(10);
    loop {
        let mut byte = (len & 0x7F) as u8;
        len >>= 7;
        if len != 0 {
            byte |= 0x80;
        }
        varint_buf.push(byte);
        if len == 0 {
            break;
        }
    }
    stream.write_all(&varint_buf).await?;
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}
