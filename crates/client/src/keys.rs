use ed25519_dalek::SigningKey;
use eyre::{Result, WrapErr};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tinycomet_types::address_from_pubkey;

#[derive(Serialize, Deserialize)]
pub struct KeyFile {
    pub secret_key: String,
    pub public_key: String,
    pub address: String,
}

pub fn generate(output: &str) -> Result<()> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let address = address_from_pubkey(&verifying_key.to_bytes());

    let key_file = KeyFile {
        secret_key: hex::encode(signing_key.to_bytes()),
        public_key: hex::encode(verifying_key.to_bytes()),
        address: format!("{address}"),
    };

    let json = serde_json::to_string_pretty(&key_file)?;
    std::fs::write(output, &json).wrap_err_with(|| format!("failed to write {output}"))?;
    println!("address: {address}");
    println!("key saved to {output}");
    Ok(())
}

pub fn load_signing_key(path: &str) -> Result<SigningKey> {
    let json = std::fs::read_to_string(path).wrap_err_with(|| format!("failed to read {path}"))?;
    let key_file: KeyFile = serde_json::from_str(&json)?;
    let bytes = hex::decode(&key_file.secret_key)?;
    let secret: [u8; 32] = bytes
        .try_into()
        .map_err(|_| eyre::eyre!("invalid secret key length"))?;
    Ok(SigningKey::from_bytes(&secret))
}
