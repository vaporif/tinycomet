mod keys;
mod query;
mod tx;

use clap::{Parser, Subcommand};
use eyre::Result;

#[derive(Parser)]
#[command(name = "tinycomet-client")]
struct Cli {
    #[arg(long, default_value = "http://127.0.0.1:26657")]
    node: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Keygen {
        #[arg(long, default_value = "key.json")]
        output: String,
    },
    Balance {
        address: String,
    },
    CreateAccount {
        #[arg(long, default_value = "key.json")]
        key: String,
    },
    Transfer {
        #[arg(long, default_value = "key.json")]
        key: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: u128,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    match cli.command {
        Command::Keygen { output } => keys::generate(&output)?,
        Command::Balance { address } => query::balance(&cli.node, &address).await?,
        Command::CreateAccount { key } => tx::create_account(&cli.node, &key).await?,
        Command::Transfer { key, to, amount } => {
            tx::transfer(&cli.node, &key, &to, amount).await?
        }
    }

    Ok(())
}
