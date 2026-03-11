use eyre::Result;
use tinycomet_types::Account;

pub async fn balance(node: &str, address: &str) -> Result<()> {
    let path = format!("account/{address}");
    let url = format!("{node}/abci_query?path=\"{path}\"");
    let resp: serde_json::Value = reqwest::get(&url).await?.json().await?;

    let code = resp["result"]["response"]["code"].as_u64().unwrap_or(0);
    if code != 0 {
        let log = resp["result"]["response"]["log"].as_str().unwrap_or("");
        println!("account not found: {log}");
        return Ok(());
    }

    let value_b64 = resp["result"]["response"]["value"].as_str().unwrap_or("");
    if value_b64.is_empty() {
        println!("account not found");
        return Ok(());
    }

    let engine = base64::engine::general_purpose::STANDARD;
    let bytes = base64::Engine::decode(&engine, value_b64)?;
    let account: Account = borsh::from_slice(&bytes)?;

    println!("address: {address}");
    println!("balance: {}", account.balance);
    println!("nonce:   {}", account.nonce);
    Ok(())
}
