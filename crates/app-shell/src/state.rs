use borsh::BorshDeserialize;
use eyre::{Result, WrapErr};
use jmt::storage::TreeWriter;
use jmt::{KeyHash, Sha256Jmt};
use sha2::Sha256;
use std::collections::HashMap;
use tinycomet_types::{Account, Address, ChainId};

use crate::storage::Storage;

pub struct State {
    pub storage: Storage,
    pub chain_id: ChainId,
    pub current_height: u64,
    pub last_app_hash: Vec<u8>,
    pub pending_writes: HashMap<Address, Account>,
}

impl State {
    pub fn new(storage: Storage) -> Result<Self> {
        let (height, app_hash) = storage.get_last_committed()?;
        Ok(Self {
            storage,
            chain_id: ChainId(String::new()),
            current_height: height,
            last_app_hash: app_hash,
            pending_writes: HashMap::new(),
        })
    }

    pub fn get_account(&self, address: &Address) -> Result<Option<Account>> {
        if let Some(account) = self.pending_writes.get(address) {
            return Ok(Some(account.clone()));
        }
        if self.current_height == 0 {
            return Ok(None);
        }
        let tree = Sha256Jmt::new(&self.storage);
        let key_hash = KeyHash::with::<Sha256>(address);
        match tree
            .get(key_hash, self.current_height)
            .map_err(|e| eyre::eyre!(e))?
        {
            Some(bytes) => {
                let account =
                    Account::try_from_slice(&bytes).wrap_err("failed to deserialize account")?;
                Ok(Some(account))
            }
            None => Ok(None),
        }
    }

    pub fn commit(&mut self) -> Result<Vec<u8>> {
        let tree = Sha256Jmt::new(&self.storage);
        let value_set: Vec<(KeyHash, Option<jmt::OwnedValue>)> = self
            .pending_writes
            .drain()
            .map(|(address, account)| {
                let key_hash = KeyHash::with::<Sha256>(&address);
                let value = borsh::to_vec(&account).expect("account serialization cannot fail");
                (key_hash, Some(value))
            })
            .collect();

        let version = self.current_height;

        if value_set.is_empty() {
            let app_hash = if self.last_app_hash.is_empty() {
                vec![0u8; 32]
            } else {
                self.last_app_hash.clone()
            };
            self.storage.set_last_committed(version, &app_hash)?;
            self.last_app_hash = app_hash.clone();
            return Ok(app_hash);
        }

        let (root_hash, tree_update_batch) = tree
            .put_value_set(value_set, version)
            .map_err(|e| eyre::eyre!(e))?;
        self.storage
            .write_node_batch(&tree_update_batch.node_batch)
            .map_err(|e| eyre::eyre!(e))?;

        let app_hash = root_hash.0.to_vec();
        self.storage.set_last_committed(version, &app_hash)?;
        self.last_app_hash = app_hash.clone();
        Ok(app_hash)
    }
}
