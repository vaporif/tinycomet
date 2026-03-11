use thiserror::Error;
use tinycomet_types::*;

use crate::state::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TxErrorCode {
    DeserializeFailed = 1,
    InvalidExpiration = 2,
    Expired = 3,
    InvalidNonce = 4,
    StorageError = 5,
    AccountAlreadyExists = 6,
    SenderNotFound = 7,
    NonceMismatch = 8,
    InsufficientBalance = 9,
    RecipientNotFound = 10,
    SignatureInvalid = 11,
}

#[derive(Error, Debug)]
#[error("tx error (code {code}): {log}")]
pub struct TxError {
    pub code: u32,
    pub log: String,
}

impl TxError {
    fn new(code: TxErrorCode, log: impl Into<String>) -> Self {
        Self {
            code: code as u32,
            log: log.into(),
        }
    }
}

impl State {
    pub fn handle_info(&self) -> AppResponse {
        AppResponse::Info {
            last_block_height: self.current_height,
            last_block_app_hash: self.last_app_hash.clone(),
        }
    }

    pub fn handle_init_chain(&mut self, chain_id: ChainId, app_state: &[u8]) -> AppResponse {
        if !self.chain_id.0.is_empty() {
            return AppResponse::InitChain {
                app_hash: self.last_app_hash.clone(),
            };
        }
        self.chain_id = chain_id;

        if !app_state.is_empty() {
            match serde_json::from_slice::<GenesisAppState>(app_state) {
                Ok(genesis) => {
                    for acct in &genesis.accounts {
                        match hex::decode(&acct.address)
                            .ok()
                            .and_then(|bytes| Address::try_from(bytes.as_slice()).ok())
                        {
                            Some(addr) => {
                                self.pending_writes.insert(
                                    addr,
                                    Account {
                                        balance: acct.balance,
                                        nonce: 0,
                                    },
                                );
                                tracing::info!(
                                    "genesis account {} balance={}",
                                    acct.address,
                                    acct.balance
                                );
                            }
                            None => {
                                tracing::warn!(
                                    "skipping invalid genesis address: {}",
                                    acct.address
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to parse genesis app_state: {e}");
                }
            }
        }

        AppResponse::InitChain {
            app_hash: self.last_app_hash.clone(),
        }
    }

    pub fn handle_check_tx(&self, tx_bytes: &[u8]) -> AppResponse {
        match self.parse_and_validate(tx_bytes) {
            Ok(_) => AppResponse::CheckTx {
                code: 0,
                log: String::new(),
            },
            Err(e) => AppResponse::CheckTx {
                code: e.code,
                log: e.log,
            },
        }
    }

    pub fn handle_prepare_proposal(&self, txs: Vec<Vec<u8>>, _max_tx_bytes: i64) -> AppResponse {
        AppResponse::PrepareProposal { txs }
    }

    pub fn handle_process_proposal(&self, txs: &[Vec<u8>]) -> AppResponse {
        for tx_bytes in txs {
            if self.parse_and_validate(tx_bytes).is_err() {
                return AppResponse::ProcessProposal { accepted: false };
            }
        }
        AppResponse::ProcessProposal { accepted: true }
    }

    pub fn handle_finalize_block(
        &mut self,
        txs: Vec<Vec<u8>>,
        height: u64,
        _time: DateTimeUtc,
    ) -> AppResponse {
        self.current_height = height;
        let mut tx_results = Vec::with_capacity(txs.len());
        for tx_bytes in &txs {
            let result = self
                .parse_and_validate(tx_bytes)
                .and_then(|(sender, tx)| self.execute(&sender, &tx));
            match result {
                Ok(()) => tx_results.push(TxResult {
                    code: 0,
                    log: String::new(),
                }),
                Err(e) => tx_results.push(TxResult {
                    code: e.code,
                    log: e.log,
                }),
            }
        }
        AppResponse::FinalizeBlock { tx_results }
    }

    pub fn handle_commit(&mut self) -> AppResponse {
        match self.commit() {
            Ok(app_hash) => AppResponse::Commit { app_hash },
            Err(e) => {
                tracing::error!("commit failed: {e:#}");
                std::process::exit(1);
            }
        }
    }

    pub fn handle_query(&self, path: &str, _data: &[u8]) -> AppResponse {
        let parts: Vec<&str> = path.splitn(2, '/').collect();
        match parts.as_slice() {
            ["account", hex_addr] => self.query_account(hex_addr),
            _ => AppResponse::Query {
                code: 1,
                value: vec![],
                log: format!("unknown query path: {path}"),
            },
        }
    }

    fn query_account(&self, hex_addr: &str) -> AppResponse {
        let address = match hex::decode(hex_addr)
            .ok()
            .and_then(|bytes| Address::try_from(bytes.as_slice()).ok())
        {
            Some(addr) => addr,
            None => {
                return AppResponse::Query {
                    code: 2,
                    value: vec![],
                    log: "invalid address hex".to_string(),
                }
            }
        };
        match self.get_account(&address) {
            Ok(Some(account)) => AppResponse::Query {
                code: 0,
                value: borsh::to_vec(&account).expect("serialize"),
                log: String::new(),
            },
            Ok(None) => AppResponse::Query {
                code: 1,
                value: vec![],
                log: "account not found".to_string(),
            },
            Err(e) => AppResponse::Query {
                code: 3,
                value: vec![],
                log: format!("query error: {e:#}"),
            },
        }
    }

    fn parse_and_validate(&self, tx_bytes: &[u8]) -> Result<(Address, Transaction), TxError> {
        let signed: SignedTransaction = borsh::from_slice(tx_bytes).map_err(|e| {
            TxError::new(
                TxErrorCode::DeserializeFailed,
                format!("failed to parse signed transaction: {e}"),
            )
        })?;

        signed.verify().map_err(|e| {
            TxError::new(
                TxErrorCode::SignatureInvalid,
                format!("invalid signature: {e}"),
            )
        })?;

        let sender = signed.sender_address();

        let tx: Transaction = borsh::from_slice(&signed.payload).map_err(|e| {
            TxError::new(
                TxErrorCode::DeserializeFailed,
                format!("failed to parse transaction payload: {e}"),
            )
        })?;

        if let Some(ref expiration) = tx.header.expiration {
            let exp = expiration.to_chrono().map_err(|e| {
                TxError::new(
                    TxErrorCode::InvalidExpiration,
                    format!("invalid expiration: {e}"),
                )
            })?;
            if exp < chrono::Utc::now() {
                return Err(TxError::new(TxErrorCode::Expired, "transaction expired"));
            }
        }

        match &tx.tx_payload {
            TxPayload::CreateAccount => {
                if tx.nonce != 1 {
                    return Err(TxError::new(
                        TxErrorCode::InvalidNonce,
                        format!("CreateAccount nonce must be 1, got {}", tx.nonce),
                    ));
                }
                if self.get_account(&sender).map_err(storage_err)?.is_some() {
                    return Err(TxError::new(
                        TxErrorCode::AccountAlreadyExists,
                        format!("account {} already exists", sender),
                    ));
                }
            }
            TxPayload::Transfer { to, amount } => {
                let acct = self
                    .get_account(&sender)
                    .map_err(storage_err)?
                    .ok_or_else(|| {
                        TxError::new(
                            TxErrorCode::SenderNotFound,
                            format!("sender {} does not exist", sender),
                        )
                    })?;
                if tx.nonce != acct.nonce + 1 {
                    return Err(TxError::new(
                        TxErrorCode::NonceMismatch,
                        format!(
                            "nonce mismatch: expected {}, got {}",
                            acct.nonce + 1,
                            tx.nonce
                        ),
                    ));
                }
                if acct.balance < amount.get() {
                    return Err(TxError::new(
                        TxErrorCode::InsufficientBalance,
                        format!(
                            "insufficient balance: have {}, need {}",
                            acct.balance,
                            amount.get()
                        ),
                    ));
                }
                if self.get_account(to).map_err(storage_err)?.is_none() {
                    return Err(TxError::new(
                        TxErrorCode::RecipientNotFound,
                        format!("recipient {} does not exist", to),
                    ));
                }
            }
        }
        Ok((sender, tx))
    }

    fn execute(&mut self, sender: &Address, tx: &Transaction) -> Result<(), TxError> {
        match &tx.tx_payload {
            TxPayload::CreateAccount => {
                let account = Account {
                    balance: 1_000_000,
                    nonce: 1,
                };
                self.pending_writes.insert(*sender, account);
            }
            TxPayload::Transfer { to, amount } => {
                let mut acct = self
                    .get_account(sender)
                    .map_err(storage_err)?
                    .ok_or_else(|| TxError::new(TxErrorCode::SenderNotFound, "sender not found"))?;
                let mut recipient =
                    self.get_account(to).map_err(storage_err)?.ok_or_else(|| {
                        TxError::new(TxErrorCode::RecipientNotFound, "recipient not found")
                    })?;
                acct.balance -= amount.get();
                acct.nonce = tx.nonce;
                recipient.balance += amount.get();
                self.pending_writes.insert(*sender, acct);
                self.pending_writes.insert(*to, recipient);
            }
        }
        Ok(())
    }
}

fn storage_err(e: eyre::Report) -> TxError {
    TxError::new(TxErrorCode::StorageError, format!("storage error: {e:#}"))
}
