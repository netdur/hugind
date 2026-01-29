use clap::Parser;
use hugind::cli::{
    agent,
    args::{AgentCommand, Cli, Commands},
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { command } => {
            let result = match command {
                AgentCommand::Run { path } => agent::run(path).await,
                AgentCommand::Install => agent::install(),
                AgentCommand::Remove => agent::remove(),
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
