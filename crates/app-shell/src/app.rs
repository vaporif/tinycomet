use thiserror::Error;
use tinycomet_types::*;

use crate::state::State;

#[derive(Error, Debug)]
#[error("validation error (code {code}): {log}")]
pub struct ValidationError {
    pub code: u32,
    pub log: String,
}

impl State {
    pub fn handle_info(&self) -> AppResponse {
        AppResponse::Info {
            last_block_height: self.current_height,
            last_block_app_hash: self.last_app_hash.clone(),
        }
    }

    pub fn handle_init_chain(&mut self, chain_id: ChainId, _initial_height: u64) -> AppResponse {
        if !self.chain_id.0.is_empty() {
            return AppResponse::InitChain {
                app_hash: self.last_app_hash.clone(),
            };
        }
        self.chain_id = chain_id;
        AppResponse::InitChain {
            app_hash: self.last_app_hash.clone(),
        }
    }

    pub fn handle_check_tx(&self, tx_bytes: &[u8]) -> AppResponse {
        match self.validate_tx(tx_bytes) {
            Ok(()) => AppResponse::CheckTx {
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
            if self.validate_tx(tx_bytes).is_err() {
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
            match self.execute_tx(tx_bytes) {
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
        let address: Address = match hex::decode(hex_addr) {
            Ok(bytes) if bytes.len() == 20 => {
                let mut addr = [0u8; 20];
                addr.copy_from_slice(&bytes);
                addr
            }
            _ => {
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

    fn validate_tx(&self, tx_bytes: &[u8]) -> std::result::Result<(), ValidationError> {
        let tx: Transaction = borsh::from_slice(tx_bytes).map_err(|e| ValidationError {
            code: 1,
            log: format!("failed to parse transaction: {e}"),
        })?;

        if let Some(ref expiration) = tx.header.expiration {
            let exp = expiration.to_chrono().map_err(|e| ValidationError {
                code: 2,
                log: format!("invalid expiration: {e}"),
            })?;
            if exp < chrono::Utc::now() {
                return Err(ValidationError {
                    code: 3,
                    log: "transaction expired".to_string(),
                });
            }
        }

        match &tx.tx_payload {
            TxPayload::CreateAccount => {
                if tx.nonce != 1 {
                    return Err(ValidationError {
                        code: 4,
                        log: format!("CreateAccount nonce must be 1, got {}", tx.nonce),
                    });
                }
                let existing = self.get_account(&tx.from).map_err(|e| ValidationError {
                    code: 5,
                    log: format!("storage error: {e:#}"),
                })?;
                if existing.is_some() {
                    return Err(ValidationError {
                        code: 6,
                        log: format!("account {} already exists", hex::encode(tx.from)),
                    });
                }
            }
            TxPayload::Transfer { to, amount } => {
                let sender = self.get_account(&tx.from).map_err(|e| ValidationError {
                    code: 5,
                    log: format!("storage error: {e:#}"),
                })?;
                let sender = sender.ok_or_else(|| ValidationError {
                    code: 7,
                    log: format!("sender {} does not exist", hex::encode(tx.from)),
                })?;
                if tx.nonce != sender.nonce + 1 {
                    return Err(ValidationError {
                        code: 8,
                        log: format!(
                            "nonce mismatch: expected {}, got {}",
                            sender.nonce + 1,
                            tx.nonce
                        ),
                    });
                }
                if sender.balance < amount.get() {
                    return Err(ValidationError {
                        code: 9,
                        log: format!(
                            "insufficient balance: have {}, need {}",
                            sender.balance,
                            amount.get()
                        ),
                    });
                }
                let recipient = self.get_account(to).map_err(|e| ValidationError {
                    code: 5,
                    log: format!("storage error: {e:#}"),
                })?;
                if recipient.is_none() {
                    return Err(ValidationError {
                        code: 10,
                        log: format!("recipient {} does not exist", hex::encode(to)),
                    });
                }
            }
        }
        Ok(())
    }

    fn execute_tx(&mut self, tx_bytes: &[u8]) -> std::result::Result<(), ValidationError> {
        let tx: Transaction = borsh::from_slice(tx_bytes).map_err(|e| ValidationError {
            code: 1,
            log: format!("failed to parse transaction: {e}"),
        })?;

        match &tx.tx_payload {
            TxPayload::CreateAccount => {
                let account = Account {
                    balance: 1_000_000,
                    nonce: 1,
                };
                self.pending_writes.insert(tx.from, account);
            }
            TxPayload::Transfer { to, amount } => {
                let mut sender = self
                    .get_account(&tx.from)
                    .map_err(|e| ValidationError {
                        code: 5,
                        log: format!("storage error: {e:#}"),
                    })?
                    .ok_or_else(|| ValidationError {
                        code: 7,
                        log: "sender not found".to_string(),
                    })?;
                let mut recipient = self
                    .get_account(to)
                    .map_err(|e| ValidationError {
                        code: 5,
                        log: format!("storage error: {e:#}"),
                    })?
                    .ok_or_else(|| ValidationError {
                        code: 10,
                        log: "recipient not found".to_string(),
                    })?;
                sender.balance -= amount.get();
                sender.nonce = tx.nonce;
                recipient.balance += amount.get();
                self.pending_writes.insert(tx.from, sender);
                self.pending_writes.insert(*to, recipient);
            }
        }
        Ok(())
    }
}
