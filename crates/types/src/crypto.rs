use borsh::{BorshDeserialize, BorshSerialize};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::domain::{Address, ADDRESS_LENGTH};

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct SignedTransaction {
    pub payload: Vec<u8>,
    pub signature: [u8; 64],
    pub public_key: [u8; 32],
}

impl SignedTransaction {
    pub fn verify(&self) -> Result<(), ed25519_dalek::SignatureError> {
        let vk = VerifyingKey::from_bytes(&self.public_key)?;
        let sig = Signature::from_bytes(&self.signature);
        vk.verify_strict(&self.payload, &sig)
    }

    pub fn sender_address(&self) -> Address {
        address_from_pubkey(&self.public_key)
    }
}

pub fn address_from_pubkey(pubkey: &[u8; 32]) -> Address {
    let hash = Sha256::digest(pubkey);
    let mut addr = [0u8; ADDRESS_LENGTH];
    addr.copy_from_slice(&hash[..ADDRESS_LENGTH]);
    Address(addr)
}
