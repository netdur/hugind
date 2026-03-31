use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },

    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },

    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },

    Server {
        #[command(subcommand)]
        command: ServerCommand,
    },

    Stdio,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    List,

    Validate {
        #[arg(default_value = "config.yaml")]
        path: String,
    },

    Info,

    Init {
        name: String,

        #[arg(short, long)]
        model: Option<String>,
    },

    Remove {
        name: String,
    },

    Defaults {
        #[arg(long)]
        hf_token: Option<String>,

        /// Set a key=value pair (can be repeated)
        #[arg(long = "set")]
        set: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum AgentCommand {
    Run {
        path: String,

        #[arg(long)]
        cwd: Option<String>,

        #[arg(long)]
        log_file: Option<String>,

        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    Install {
        path: String,
    },

    Remove {
        name: String,
    },

    List,
}

#[derive(Subcommand, Debug)]
pub enum ModelCommand {
    List,

    Add {
        repo: Option<String>,

        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    Show { repo: String },

    Remove {
        repo: Option<String>,

        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Migrate models from old layout (~/.hugind/{user}/{repo}) to new (~/.hugind/models/{user}/{repo})
    Migrate,
}

#[derive(Subcommand, Debug)]
pub enum ServerCommand {
    Start {
        config: String,

        #[arg(short, long)]
        port: Option<u16>,
    },

    List,

    Stop {
        config: String,
    },
}
