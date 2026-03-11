use borsh::{BorshDeserialize, BorshSerialize};
use std::fmt;
use std::num::NonZeroU128;

pub const ADDRESS_LENGTH: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct Address(pub [u8; ADDRESS_LENGTH]);

impl Address {
    pub fn as_bytes(&self) -> &[u8; ADDRESS_LENGTH] {
        &self.0
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl TryFrom<&[u8]> for Address {
    type Error = std::array::TryFromSliceError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let arr: [u8; ADDRESS_LENGTH] = bytes.try_into()?;
        Ok(Self(arr))
    }
}

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
    pub tx_payload: TxPayload,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TxPayload {
    CreateAccount,
    Transfer { to: Address, amount: NonZeroU128 },
}
