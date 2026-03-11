use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use eyre::{Result, WrapErr};
use frost_ed25519 as frost;
use rand::rngs::OsRng;
use tinycomet_types::address_from_pubkey;

pub fn dkg(threshold: u16, participants: u16, output_dir: &str) -> Result<()> {
    let rng = OsRng;

    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        participants,
        threshold,
        frost::keys::IdentifierList::Default,
        rng,
    )
    .map_err(|e| eyre::eyre!("DKG failed: {e}"))?;

    std::fs::create_dir_all(output_dir)
        .wrap_err_with(|| format!("failed to create {output_dir}"))?;

    let group_key = pubkey_package.verifying_key();
    let group_key_bytes: [u8; 32] = group_key
        .serialize()
        .map_err(|e| eyre::eyre!("failed to serialize group key: {e}"))?
        .try_into()
        .map_err(|_| eyre::eyre!("unexpected group key length"))?;
    let address = address_from_pubkey(&group_key_bytes);

    let pubkey_json = serde_json::to_string_pretty(&pubkey_package)?;
    std::fs::write(
        format!("{output_dir}/public_key_package.json"),
        &pubkey_json,
    )?;

    for (id, secret_share) in &shares {
        let share_json = serde_json::to_string_pretty(secret_share)?;
        let id_bytes = id.serialize();
        let id_hex = hex::encode(id_bytes);
        let share_path = format!("{output_dir}/share-{id_hex}.json");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut file = opts
            .open(&share_path)
            .wrap_err_with(|| format!("failed to write {share_path}"))?;
        file.write_all(share_json.as_bytes())?;
    }

    println!("FROST {threshold}-of-{participants} key generated");
    println!("group address: {address}");
    println!("shares saved to {output_dir}/");
    Ok(())
}
