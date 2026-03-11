use anyhow::Context as _;
use borsh::BorshDeserialize;
use jmt::storage::{LeafNode, Node, NodeBatch, NodeKey, TreeReader, TreeWriter};
use jmt::{KeyHash, OwnedValue, Version};
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use std::path::Path;
use std::sync::Arc;

const CF_JMT_NODES: &str = "jmt_nodes";
const CF_JMT_VALUES: &str = "jmt_values";
const CF_META: &str = "meta";

const META_LAST_HEIGHT: &[u8] = b"last_height";
const META_LAST_APP_HASH: &[u8] = b"last_app_hash";

pub struct Storage {
    db: Arc<DB>,
}

impl Storage {
    pub fn open(path: &Path) -> eyre::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf_nodes = ColumnFamilyDescriptor::new(CF_JMT_NODES, Options::default());
        let cf_values = ColumnFamilyDescriptor::new(CF_JMT_VALUES, Options::default());
        let cf_meta = ColumnFamilyDescriptor::new(CF_META, Options::default());

        let db = DB::open_cf_descriptors(&opts, path, vec![cf_nodes, cf_values, cf_meta])
            .map_err(|e| eyre::eyre!("failed to open RocksDB: {e}"))?;

        Ok(Self { db: Arc::new(db) })
    }

    pub fn get_last_committed(&self) -> eyre::Result<(u64, Vec<u8>)> {
        let cf_meta = self.db.cf_handle(CF_META).expect("meta cf must exist");

        let height = self
            .db
            .get_cf(&cf_meta, META_LAST_HEIGHT)
            .map_err(|e| eyre::eyre!("rocksdb get last height: {e}"))?
            .map(|bytes| {
                let arr: [u8; 8] = bytes.try_into().expect("height must be 8 bytes");
                u64::from_le_bytes(arr)
            })
            .unwrap_or(0);

        let app_hash = self
            .db
            .get_cf(&cf_meta, META_LAST_APP_HASH)
            .map_err(|e| eyre::eyre!("rocksdb get last app hash: {e}"))?
            .unwrap_or_default();

        Ok((height, app_hash))
    }

    pub fn set_last_committed(&self, height: u64, app_hash: &[u8]) -> eyre::Result<()> {
        let cf_meta = self.db.cf_handle(CF_META).expect("meta cf must exist");
        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(&cf_meta, META_LAST_HEIGHT, height.to_le_bytes());
        batch.put_cf(&cf_meta, META_LAST_APP_HASH, app_hash);
        self.db
            .write(batch)
            .map_err(|e| eyre::eyre!("failed to write meta: {e}"))?;
        Ok(())
    }

    fn node_key_to_bytes(node_key: &NodeKey) -> Vec<u8> {
        borsh::to_vec(node_key).expect("NodeKey serialization cannot fail")
    }

    fn value_key_to_bytes(version: Version, key_hash: KeyHash) -> Vec<u8> {
        let mut buf = Vec::with_capacity(8 + 32);
        buf.extend_from_slice(&version.to_be_bytes());
        buf.extend_from_slice(&key_hash.0);
        buf
    }
}

impl TreeReader for Storage {
    fn get_node_option(&self, node_key: &NodeKey) -> anyhow::Result<Option<Node>> {
        let cf = self.db.cf_handle(CF_JMT_NODES).expect("cf must exist");
        let key = Self::node_key_to_bytes(node_key);
        match self.db.get_cf(&cf, &key).context("rocksdb get node")? {
            Some(bytes) => {
                let node =
                    Node::try_from_slice(&bytes).context("failed to deserialize jmt Node")?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    fn get_value_option(
        &self,
        max_version: Version,
        key_hash: KeyHash,
    ) -> anyhow::Result<Option<OwnedValue>> {
        let cf = self.db.cf_handle(CF_JMT_VALUES).expect("cf must exist");
        for version in (0..=max_version).rev() {
            let key = Self::value_key_to_bytes(version, key_hash);
            if let Some(bytes) = self.db.get_cf(&cf, &key).context("rocksdb get value")? {
                return Ok(Some(bytes));
            }
        }
        Ok(None)
    }

    fn get_rightmost_leaf(&self) -> anyhow::Result<Option<(NodeKey, LeafNode)>> {
        Ok(None)
    }
}

impl jmt::storage::HasPreimage for Storage {
    fn preimage(&self, _key_hash: KeyHash) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

impl TreeWriter for Storage {
    fn write_node_batch(&self, node_batch: &NodeBatch) -> anyhow::Result<()> {
        let cf_nodes = self.db.cf_handle(CF_JMT_NODES).expect("cf must exist");
        let cf_values = self.db.cf_handle(CF_JMT_VALUES).expect("cf must exist");

        let mut batch = rocksdb::WriteBatch::default();

        for (node_key, node) in node_batch.nodes() {
            let key = Self::node_key_to_bytes(node_key);
            let value = borsh::to_vec(node).context("failed to serialize jmt Node")?;
            batch.put_cf(&cf_nodes, &key, &value);
        }

        for ((version, key_hash), value) in node_batch.values() {
            let key = Self::value_key_to_bytes(*version, *key_hash);
            match value {
                Some(v) => batch.put_cf(&cf_values, &key, v),
                None => batch.delete_cf(&cf_values, &key),
            }
        }

        self.db.write(batch).context("failed to write node batch")?;

        Ok(())
    }
}
