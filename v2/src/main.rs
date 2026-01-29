use clap::Parser;
use hugind::cli::{
    agent,
    args::{AgentCommand, Cli, Commands, ConfigCommand},
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { command } => {
            let result = match command {
                AgentCommand::Run { path, args } => agent::run(path, args).await,
                AgentCommand::Install => agent::install(),
                AgentCommand::Remove => agent::remove(),
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Config { command } => {
            let result = match command {
                ConfigCommand::List => hugind::cli::config::list(),
                ConfigCommand::Validate { path } => hugind::cli::config::validate(path),
                ConfigCommand::Info => hugind::cli::config::info(),
                ConfigCommand::Remove { name } => hugind::cli::config::remove(name),
                ConfigCommand::Defaults { lib, hf_token } => hugind::cli::config::defaults(lib, hf_token),
                ConfigCommand::Init { name, model } => hugind::cli::config_init::init(name, model),
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
