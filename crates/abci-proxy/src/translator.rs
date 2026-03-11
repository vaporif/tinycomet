use tendermint_proto::v0_38::abci as pb;
use tinycomet_types::*;

pub fn abci_to_app_request(value: &pb::request::Value) -> Option<AppRequest> {
    match value {
        pb::request::Value::Info(_) => Some(AppRequest::Info),
        pb::request::Value::InitChain(req) => Some(AppRequest::InitChain {
            chain_id: ChainId(req.chain_id.clone()),
            initial_height: req.initial_height as u64,
        }),
        pb::request::Value::CheckTx(req) => Some(AppRequest::CheckTx {
            tx_bytes: req.tx.to_vec(),
        }),
        pb::request::Value::PrepareProposal(req) => Some(AppRequest::PrepareProposal {
            txs: req.txs.iter().map(|t| t.to_vec()).collect(),
            max_tx_bytes: req.max_tx_bytes,
        }),
        pb::request::Value::ProcessProposal(req) => Some(AppRequest::ProcessProposal {
            txs: req.txs.iter().map(|t| t.to_vec()).collect(),
        }),
        pb::request::Value::FinalizeBlock(req) => {
            let time = req
                .time
                .as_ref()
                .map(|t| {
                    let dt = chrono::DateTime::from_timestamp(t.seconds, t.nanos as u32)
                        .unwrap_or_default();
                    DateTimeUtc {
                        rfc3339: dt.to_rfc3339(),
                    }
                })
                .unwrap_or_else(DateTimeUtc::now);
            Some(AppRequest::FinalizeBlock {
                txs: req.txs.iter().map(|t| t.to_vec()).collect(),
                height: req.height as u64,
                time,
            })
        }
        pb::request::Value::Commit(_) => Some(AppRequest::Commit),
        pb::request::Value::Query(req) => Some(AppRequest::Query {
            path: req.path.clone(),
            data: req.data.to_vec(),
        }),
        pb::request::Value::Echo(_)
        | pb::request::Value::Flush(_)
        | pb::request::Value::ListSnapshots(_)
        | pb::request::Value::OfferSnapshot(_)
        | pb::request::Value::LoadSnapshotChunk(_)
        | pb::request::Value::ApplySnapshotChunk(_)
        | pb::request::Value::ExtendVote(_)
        | pb::request::Value::VerifyVoteExtension(_) => None,
    }
}

pub fn app_response_to_abci(response: AppResponse) -> pb::response::Value {
    match response {
        AppResponse::Info {
            last_block_height,
            last_block_app_hash,
        } => pb::response::Value::Info(pb::ResponseInfo {
            data: "tinycomet".to_string(),
            version: String::new(),
            app_version: 1,
            last_block_height: last_block_height as i64,
            last_block_app_hash: last_block_app_hash.into(),
        }),
        AppResponse::InitChain { app_hash } => {
            pb::response::Value::InitChain(pb::ResponseInitChain {
                consensus_params: None,
                validators: vec![],
                app_hash: app_hash.into(),
            })
        }
        AppResponse::CheckTx { code, log } => pb::response::Value::CheckTx(pb::ResponseCheckTx {
            code,
            log,
            ..Default::default()
        }),
        AppResponse::PrepareProposal { txs } => {
            pb::response::Value::PrepareProposal(pb::ResponsePrepareProposal {
                txs: txs.into_iter().map(Into::into).collect(),
            })
        }
        AppResponse::ProcessProposal { accepted } => {
            pb::response::Value::ProcessProposal(pb::ResponseProcessProposal {
                status: if accepted {
                    pb::response_process_proposal::ProposalStatus::Accept as i32
                } else {
                    pb::response_process_proposal::ProposalStatus::Reject as i32
                },
            })
        }
        AppResponse::FinalizeBlock { tx_results } => {
            pb::response::Value::FinalizeBlock(pb::ResponseFinalizeBlock {
                tx_results: tx_results
                    .into_iter()
                    .map(|r| pb::ExecTxResult {
                        code: r.code,
                        log: r.log,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            })
        }
        AppResponse::Commit { app_hash: _ } => {
            pb::response::Value::Commit(pb::ResponseCommit { retain_height: 0 })
        }
        AppResponse::Query { code, value, log } => pb::response::Value::Query(pb::ResponseQuery {
            code,
            log,
            value: value.into(),
            ..Default::default()
        }),
    }
}

pub fn proxy_response(value: &pb::request::Value) -> Option<pb::response::Value> {
    match value {
        pb::request::Value::Echo(req) => Some(pb::response::Value::Echo(pb::ResponseEcho {
            message: req.message.clone(),
        })),
        pb::request::Value::Flush(_) => Some(pb::response::Value::Flush(pb::ResponseFlush {})),
        pb::request::Value::ListSnapshots(_) => Some(pb::response::Value::ListSnapshots(
            pb::ResponseListSnapshots { snapshots: vec![] },
        )),
        pb::request::Value::OfferSnapshot(_) => Some(pb::response::Value::OfferSnapshot(
            pb::ResponseOfferSnapshot {
                result: pb::response_offer_snapshot::Result::Reject as i32,
            },
        )),
        pb::request::Value::LoadSnapshotChunk(_) => Some(pb::response::Value::LoadSnapshotChunk(
            pb::ResponseLoadSnapshotChunk {
                chunk: bytes::Bytes::new(),
            },
        )),
        pb::request::Value::ApplySnapshotChunk(_) => Some(pb::response::Value::ApplySnapshotChunk(
            pb::ResponseApplySnapshotChunk {
                result: pb::response_apply_snapshot_chunk::Result::Abort as i32,
                ..Default::default()
            },
        )),
        pb::request::Value::ExtendVote(_) => {
            Some(pb::response::Value::ExtendVote(pb::ResponseExtendVote {
                vote_extension: bytes::Bytes::new(),
            }))
        }
        pb::request::Value::VerifyVoteExtension(_) => Some(
            pb::response::Value::VerifyVoteExtension(pb::ResponseVerifyVoteExtension {
                status: pb::response_verify_vote_extension::VerifyStatus::Accept as i32,
            }),
        ),
        _ => None,
    }
}
