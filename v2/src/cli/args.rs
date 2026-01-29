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
    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Manage AI models
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// List saved configurations
    List,
    /// Validate configuration
    Validate {
         /// Path to config file (optional)
        #[arg(default_value = "config.yaml")]
        path: String,
    },
    /// Show system hardware info
    Info,
    /// Initialize a new config
    Init {
        /// Name of the config to create
        name: String,
        /// Path to model file (optional skip interactive picker)
        #[arg(short, long)]
        model: Option<String>,
    },
    /// Remove a saved config
    Remove {
        /// Name of the config to remove
        name: String,
    },
    /// Set global defaults
    Defaults {
        /// Set default library path
        #[arg(long)]
        lib: Option<String>,
        /// Set Hugging Face token
        #[arg(long)]
        hf_token: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentCommand {
    /// Run an agent
    Run {
        /// Path to the agent entry module
        path: String,
        /// Additional arguments passed to the agent
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Install an agent
    Install,
    /// Remove an agent
    Remove,
}

#[derive(Subcommand, Debug)]
pub enum ModelCommand {
    /// List local model repositories
    List,
    /// Download models from Hugging Face
    Add {
        /// Hugging Face repository (user/repo)
        repo: Option<String>,
    },
    /// Show local files in a repository
    Show {
         /// Model repository (user/repo)
         repo: String,
    },
    /// Remove a repository or specific files
    Remove {
         /// Model repository (user/repo)
         repo: Option<String>,
    },
}
