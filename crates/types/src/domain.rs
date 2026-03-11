use borsh::{BorshDeserialize, BorshSerialize};
use std::num::NonZeroU128;

pub const ADDRESS_LENGTH: usize = 20;
pub type Address = [u8; ADDRESS_LENGTH];

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct ChainId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Account {
    pub balance: u128,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DateTimeUtc {
    pub rfc3339: String,
}

impl DateTimeUtc {
    pub fn now() -> Self {
        Self {
            rfc3339: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn to_chrono(&self) -> Result<chrono::DateTime<chrono::Utc>, chrono::ParseError> {
        self.rfc3339.parse()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Header {
    pub chain_id: ChainId,
    pub expiration: Option<DateTimeUtc>,
    pub timestamp: DateTimeUtc,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Transaction {
    pub header: Header,
    pub from: Address,
    pub tx_payload: TxPayload,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TxPayload {
    CreateAccount,
    Transfer { to: Address, amount: NonZeroU128 },
}
