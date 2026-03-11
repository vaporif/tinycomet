mod server;

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Mutex;

use clap::Parser;
use eyre::{Result, WrapErr};
use tendermint_abci::Application;
use tendermint_proto::v0_38::abci::*;
use tinycomet_types::*;

#[derive(Parser)]
#[command(name = "tinycomet-proxy")]
struct Cli {
    #[arg(long, default_value = "/tmp/app.sock")]
    app_socket: PathBuf,
    #[arg(long, default_value = "/tmp/cmt.sock")]
    cmt_socket: PathBuf,
}

struct AbciApp {
    app_socket: PathBuf,
    conn: Mutex<Option<UnixStream>>,
}

impl Clone for AbciApp {
    fn clone(&self) -> Self {
        Self {
            app_socket: self.app_socket.clone(),
            conn: Mutex::new(None),
        }
    }
}

impl AbciApp {
    fn new(app_socket: PathBuf) -> Self {
        Self {
            app_socket,
            conn: Mutex::new(None),
        }
    }

    fn forward(&self, request: &AppRequest) -> AppResponse {
        match self.try_forward(request) {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("app-shell communication error: {e:#}");
                panic!("fatal: app-shell communication failed: {e:#}");
            }
        }
    }

    fn try_forward(&self, request: &AppRequest) -> Result<AppResponse> {
        let mut guard = self.conn.lock().unwrap();
        if guard.is_none() {
            *guard = Some(UnixStream::connect(&self.app_socket)?);
        }
        let stream = guard.as_mut().unwrap();

        let request_bytes = borsh::to_vec(request)?;
        let len = (request_bytes.len() as u32).to_le_bytes();
        stream.write_all(&len)?;
        stream.write_all(&request_bytes)?;
        stream.flush()?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf)?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; resp_len];
        stream.read_exact(&mut buf)?;

        Ok(borsh::from_slice(&buf)?)
    }
}

impl Application for AbciApp {
    fn info(&self, _request: RequestInfo) -> ResponseInfo {
        let AppResponse::Info {
            last_block_height,
            last_block_app_hash,
        } = self.forward(&AppRequest::Info)
        else {
            panic!("unexpected response type for Info");
        };
        ResponseInfo {
            data: "tinycomet".to_string(),
            app_version: 1,
            last_block_height: last_block_height as i64,
            last_block_app_hash: last_block_app_hash.into(),
            ..Default::default()
        }
    }

    fn init_chain(&self, request: RequestInitChain) -> ResponseInitChain {
        let AppResponse::InitChain { app_hash } = self.forward(&AppRequest::InitChain {
            chain_id: ChainId(request.chain_id),
            initial_height: request.initial_height as u64,
            app_state: request.app_state_bytes.to_vec(),
        }) else {
            panic!("unexpected response type for InitChain");
        };
        ResponseInitChain {
            app_hash: app_hash.into(),
            ..Default::default()
        }
    }

    fn query(&self, request: RequestQuery) -> ResponseQuery {
        let AppResponse::Query { code, value, log } = self.forward(&AppRequest::Query {
            path: request.path,
            data: request.data.to_vec(),
        }) else {
            panic!("unexpected response type for Query");
        };
        ResponseQuery {
            code,
            log,
            value: value.into(),
            ..Default::default()
        }
    }

    fn check_tx(&self, request: RequestCheckTx) -> ResponseCheckTx {
        let AppResponse::CheckTx { code, log } = self.forward(&AppRequest::CheckTx {
            tx_bytes: request.tx.to_vec(),
        }) else {
            panic!("unexpected response type for CheckTx");
        };
        ResponseCheckTx {
            code,
            log,
            ..Default::default()
        }
    }

    fn commit(&self) -> ResponseCommit {
        self.forward(&AppRequest::Commit);
        ResponseCommit { retain_height: 0 }
    }

    fn prepare_proposal(&self, request: RequestPrepareProposal) -> ResponsePrepareProposal {
        let AppResponse::PrepareProposal { txs } = self.forward(&AppRequest::PrepareProposal {
            txs: request.txs.into_iter().map(|t| t.to_vec()).collect(),
            max_tx_bytes: request.max_tx_bytes,
        }) else {
            panic!("unexpected response type for PrepareProposal");
        };
        ResponsePrepareProposal {
            txs: txs.into_iter().map(Into::into).collect(),
        }
    }

    fn process_proposal(&self, request: RequestProcessProposal) -> ResponseProcessProposal {
        let AppResponse::ProcessProposal { accepted } =
            self.forward(&AppRequest::ProcessProposal {
                txs: request.txs.into_iter().map(|t| t.to_vec()).collect(),
            })
        else {
            panic!("unexpected response type for ProcessProposal");
        };
        ResponseProcessProposal {
            status: if accepted {
                response_process_proposal::ProposalStatus::Accept as i32
            } else {
                response_process_proposal::ProposalStatus::Reject as i32
            },
        }
    }

    fn finalize_block(&self, request: RequestFinalizeBlock) -> ResponseFinalizeBlock {
        let time = request
            .time
            .as_ref()
            .map(|t| {
                let dt =
                    chrono::DateTime::from_timestamp(t.seconds, t.nanos as u32).unwrap_or_default();
                DateTimeUtc {
                    rfc3339: dt.to_rfc3339(),
                }
            })
            .unwrap_or_else(DateTimeUtc::now);

        let AppResponse::FinalizeBlock { tx_results } = self.forward(&AppRequest::FinalizeBlock {
            txs: request.txs.into_iter().map(|t| t.to_vec()).collect(),
            height: request.height as u64,
            time,
        }) else {
            panic!("unexpected response type for FinalizeBlock");
        };

        ResponseFinalizeBlock {
            tx_results: tx_results
                .into_iter()
                .map(|r| ExecTxResult {
                    code: r.code,
                    log: r.log,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    if !cli.app_socket.exists() {
        eyre::bail!(
            "app socket {} does not exist — start tinycomet-app first",
            cli.app_socket.display()
        );
    }

    if cli.cmt_socket.exists() {
        std::fs::remove_file(&cli.cmt_socket).wrap_err("failed to remove stale cmt socket")?;
    }

    let listener = UnixListener::bind(&cli.cmt_socket)
        .wrap_err_with(|| format!("failed to bind {}", cli.cmt_socket.display()))?;

    tracing::info!(
        "proxy listening on {} -> forwarding to {}",
        cli.cmt_socket.display(),
        cli.app_socket.display()
    );

    let app = AbciApp::new(cli.app_socket);
    server::serve(listener, app);

    Ok(())
}
