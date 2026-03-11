use std::num::NonZeroU128;

use ed25519_dalek::Signer;
use eyre::{Result, WrapErr};
use tinycomet_types::*;

use crate::keys::load_signing_key;

fn sign_transaction(key_path: &str, tx: &Transaction) -> Result<Vec<u8>> {
    let signing_key = load_signing_key(key_path)?;
    let payload = borsh::to_vec(tx)?;
    let signature = signing_key.sign(&payload);

    let signed = SignedTransaction {
        payload,
        signature: signature.to_bytes(),
        public_key: signing_key.verifying_key().to_bytes(),
    };

    Ok(borsh::to_vec(&signed)?)
}

async fn broadcast(node: &str, tx_bytes: &[u8]) -> Result<()> {
    let encoded = hex::encode(tx_bytes);
    let url = format!("{node}/broadcast_tx_commit?tx=0x{encoded}");
    let resp: serde_json::Value = reqwest::get(&url)
        .await
        .wrap_err("broadcast failed")?
        .json()
        .await?;

    if let Some(error) = resp.get("error") {
        eyre::bail!("RPC error: {error}");
    }

    let result = &resp["result"];
    let check_code = result["check_tx"]["code"].as_u64().unwrap_or(0);
    if check_code != 0 {
        let log = result["check_tx"]["log"].as_str().unwrap_or("");
        eyre::bail!("check_tx failed (code {check_code}): {log}");
    }

    let deliver_code = result["deliver_tx"]["code"].as_u64().unwrap_or(0);
    if deliver_code != 0 {
        let log = result["deliver_tx"]["log"].as_str().unwrap_or("");
        eyre::bail!("deliver_tx failed (code {deliver_code}): {log}");
    }

    println!("tx committed successfully");
    Ok(())
}

fn make_header() -> Header {
    Header {
        chain_id: ChainId("test-chain".to_string()),
        expiration: None,
        timestamp: DateTimeUtc::now(),
    }
}

pub async fn create_account(node: &str, key_path: &str) -> Result<()> {
    let tx = Transaction {
        header: make_header(),
        tx_payload: TxPayload::CreateAccount,
        nonce: 1,
    };
    let tx_bytes = sign_transaction(key_path, &tx)?;
    broadcast(node, &tx_bytes).await
}

pub async fn transfer(node: &str, key_path: &str, to_hex: &str, amount: u128) -> Result<()> {
    let to_bytes = hex::decode(to_hex).wrap_err("invalid recipient address hex")?;
    let to = Address::try_from(to_bytes.as_slice())
        .map_err(|_| eyre::eyre!("invalid address length"))?;
    let amount =
        NonZeroU128::new(amount).ok_or_else(|| eyre::eyre!("amount must be > 0"))?;

    let signing_key = load_signing_key(key_path)?;
    let sender = address_from_pubkey(&signing_key.verifying_key().to_bytes());

    let nonce = query_nonce(node, &format!("{sender}")).await? + 1;

    let tx = Transaction {
        header: make_header(),
        tx_payload: TxPayload::Transfer { to, amount },
        nonce,
    };
    let tx_bytes = sign_transaction(key_path, &tx)?;
    broadcast(node, &tx_bytes).await
}

async fn query_nonce(node: &str, address: &str) -> Result<u64> {
    let path = format!("account/{address}");
    let url = format!("{node}/abci_query?path=\"{path}\"");
    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;

    let value_b64 = resp["result"]["response"]["value"]
        .as_str()
        .unwrap_or("");
    if value_b64.is_empty() {
        return Ok(0);
    }
    let bytes = base64_decode(value_b64)?;
    let account: Account = borsh::from_slice(&bytes)?;
    Ok(account.nonce)
}

pub fn genesis_init(key_path: &str, balance: u128, genesis_path: &str) -> Result<()> {
    let signing_key = load_signing_key(key_path)?;
    let address = address_from_pubkey(&signing_key.verifying_key().to_bytes());

    let mut genesis: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(genesis_path)?)?;

    let app_state = serde_json::json!({
        "accounts": [{
            "address": format!("{address}"),
            "balance": balance,
        }]
    });

    genesis["app_state"] = app_state;
    std::fs::write(genesis_path, serde_json::to_string_pretty(&genesis)?)?;
    println!("genesis updated with account {address} balance={balance}");
    Ok(())
}

fn base64_decode(input: &str) -> Result<Vec<u8>> {
    let engine = base64::engine::general_purpose::STANDARD;
    base64::Engine::decode(&engine, input).wrap_err("base64 decode failed")
}
