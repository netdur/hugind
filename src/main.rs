use clap::Parser;
use hugind::cli::{
    agent,
    args::{AgentCommand, Cli, Commands, ConfigCommand},
};

#[tokio::main]
async fn main() {
    if let Err(e) = hugind::shared::bootstrap::ensure_user_home() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { command } => {
            let result = match command {
                AgentCommand::Run {
                    path,
                    cwd,
                    log_file,
                    args,
                } => agent::run(path, cwd, log_file, args).await,
                AgentCommand::Install { path } => agent::install(path).await,
                AgentCommand::Remove { name } => agent::remove(name),
                AgentCommand::List => agent::list(),
                AgentCommand::Team {
                    goal,
                    goal_file,
                    agents,
                    backend,
                    concurrency,
                } => {
                    let goal = match (goal, goal_file) {
                        (Some(g), None) => Ok(g),
                        (None, Some(path)) => std::fs::read_to_string(&path)
                            .map_err(|e| anyhow::anyhow!("Failed to read goal file '{}': {}", path.display(), e)),
                        (Some(_), Some(_)) => Err(anyhow::anyhow!("Provide either a goal or --goal-file, not both")),
                        (None, None) => Err(anyhow::anyhow!("Provide a goal as an argument or use --goal-file")),
                    };
                    match goal {
                        Ok(g) => agent::team(g, agents, backend, concurrency).await,
                        Err(e) => Err(e),
                    }
                },
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
                ConfigCommand::Defaults { hf_token, set } => {
                    hugind::cli::config::defaults(hf_token, set)
                }
                ConfigCommand::Init { name, model } => hugind::cli::config_init::init(name, model),
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Model { command } => {
            let result = match command {
                hugind::cli::args::ModelCommand::List => hugind::cli::model::list(),
                hugind::cli::args::ModelCommand::Add { repo, yes } => {
                    hugind::cli::model::add(repo, yes).await
                }
                hugind::cli::args::ModelCommand::Show { repo } => hugind::cli::model::show(repo),
                hugind::cli::args::ModelCommand::Remove { repo, yes } => {
                    hugind::cli::model::remove(repo, yes)
                }
                hugind::cli::args::ModelCommand::Migrate => hugind::cli::model::migrate(),
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Server { command } => {
            let result = match command {
                hugind::cli::args::ServerCommand::Start { config, port } => {
                    hugind::cli::server::run_start(config, port).await
                }
                hugind::cli::args::ServerCommand::List => hugind::cli::server::run_list().await,
                hugind::cli::args::ServerCommand::Stop { config } => {
                    hugind::cli::server::run_stop(config).await
                }
            };

            if let Err(e) = result {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Commands::Stdio => {
            if let Err(e) = hugind::stdio::run().await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}
