mod frost_cmd;
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
    GenesisInit {
        #[arg(long, default_value = "key.json")]
        key: String,
        #[arg(long, default_value_t = 1_000_000_000)]
        balance: u128,
        #[arg(long)]
        genesis_path: String,
    },
    Frost {
        #[command(subcommand)]
        frost_command: FrostCommand,
    },
}

#[derive(Subcommand)]
enum FrostCommand {
    Dkg {
        #[arg(long)]
        threshold: u16,
        #[arg(long)]
        participants: u16,
        #[arg(long, default_value = "frost-keys")]
        output_dir: String,
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
        Command::GenesisInit { key, balance, genesis_path } => {
            tx::genesis_init(&key, balance, &genesis_path)?;
        }
        Command::Frost { frost_command } => match frost_command {
            FrostCommand::Dkg {
                threshold,
                participants,
                output_dir,
            } => frost_cmd::dkg(threshold, participants, &output_dir)?,
        },
    }

    Ok(())
}
