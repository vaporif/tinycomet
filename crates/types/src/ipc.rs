use borsh::{BorshDeserialize, BorshSerialize};

use crate::domain::{ChainId, DateTimeUtc};

pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub enum AppRequest {
    Info,
    InitChain {
        chain_id: ChainId,
        initial_height: u64,
    },
    CheckTx {
        tx_bytes: Vec<u8>,
    },
    PrepareProposal {
        txs: Vec<Vec<u8>>,
        max_tx_bytes: i64,
    },
    ProcessProposal {
        txs: Vec<Vec<u8>>,
    },
    FinalizeBlock {
        txs: Vec<Vec<u8>>,
        height: u64,
        time: DateTimeUtc,
    },
    Commit,
    Query {
        path: String,
        data: Vec<u8>,
    },
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub enum AppResponse {
    Info {
        last_block_height: u64,
        last_block_app_hash: Vec<u8>,
    },
    InitChain {
        app_hash: Vec<u8>,
    },
    CheckTx {
        code: u32,
        log: String,
    },
    PrepareProposal {
        txs: Vec<Vec<u8>>,
    },
    ProcessProposal {
        accepted: bool,
    },
    FinalizeBlock {
        tx_results: Vec<TxResult>,
    },
    Commit {
        app_hash: Vec<u8>,
    },
    Query {
        code: u32,
        value: Vec<u8>,
        log: String,
    },
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct TxResult {
    pub code: u32,
    pub log: String,
}
