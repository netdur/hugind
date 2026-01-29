use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage agents
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentCommand {
    /// Run an agent
    Run {
        /// Path to the agent entry module
        path: String,
    },
    /// Install an agent
    Install,
    /// Remove an agent
    Remove,
}
