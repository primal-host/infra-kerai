mod commands;
mod config;
mod db;
mod output;

use clap::{Parser, Subcommand};
use output::OutputFormat;

#[derive(Parser)]
#[command(name = "kerai", version, about = "AST-based version control")]
struct Cli {
    /// Postgres connection string (overrides config)
    #[arg(long, global = true)]
    db: Option<String>,

    /// Config profile to use
    #[arg(long, global = true, default_value = "default")]
    profile: String,

    /// Output format
    #[arg(long, global = true, value_enum, default_value = "table")]
    format: OutputFormat,

    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Initialize a project: create config and parse crate
    Init {
        /// Path to project root (defaults to current directory)
        path: Option<String>,
    },

    /// Test connection and extension status
    Ping,

    /// Show instance info
    Info,

    /// Show CRDT version vector
    Version,

    /// Run raw SQL and format results
    Query {
        /// SQL statement to execute
        sql: String,
    },

    /// Reconstruct source files from AST
    Checkout {
        /// Reconstruct a single file by name
        #[arg(long)]
        file: Option<String>,
    },

    /// Show operation history
    Log {
        /// Filter by author
        #[arg(long)]
        author: Option<String>,

        /// Maximum number of entries
        #[arg(long, default_value = "50")]
        limit: i64,
    },

    /// Re-parse changed files
    Commit {
        /// Commit message (reserved for future use)
        #[arg(short, long)]
        message: Option<String>,
    },

    /// Manage peer instances
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },

    /// Sync CRDT operations with a peer
    Sync {
        /// Peer name to sync with
        peer: String,
    },

    /// Search AST nodes by content pattern
    Find {
        /// Search pattern (ILIKE syntax, e.g. %hello%)
        pattern: String,

        /// Filter by node kind (e.g. fn, struct, enum)
        #[arg(long)]
        kind: Option<String>,

        /// Maximum results (default 50)
        #[arg(long)]
        limit: Option<i32>,
    },

    /// Find definitions, references, and impls for a symbol
    Refs {
        /// Symbol name to search for
        symbol: String,
    },

    /// Show AST tree structure
    Tree {
        /// ltree path pattern (subtree or lquery with wildcards)
        path: Option<String>,
    },

    /// Manage AI agents
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Show an agent's perspectives
    Perspective {
        /// Agent name
        agent: String,

        /// Filter by context node ID
        #[arg(long)]
        context: Option<String>,

        /// Minimum weight threshold
        #[arg(long)]
        min_weight: Option<f64>,
    },

    /// Show multi-agent consensus
    Consensus {
        /// Filter by context node ID
        #[arg(long)]
        context: Option<String>,

        /// Minimum number of agreeing agents (default 2)
        #[arg(long)]
        min_agents: Option<i32>,

        /// Minimum average weight threshold
        #[arg(long)]
        min_weight: Option<f64>,
    },
}

#[derive(Subcommand)]
enum PeerAction {
    /// Register or update a peer
    Add {
        /// Peer name
        name: String,

        /// Ed25519 public key (hex)
        #[arg(long)]
        public_key: String,

        /// Peer endpoint URL
        #[arg(long)]
        endpoint: Option<String>,

        /// Peer Postgres connection string
        #[arg(long)]
        connection: Option<String>,
    },

    /// List all peers
    List,

    /// Remove a peer
    Remove {
        /// Peer name to remove
        name: String,
    },

    /// Show peer details
    Info {
        /// Peer name
        name: String,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Register or update an agent
    Add {
        /// Agent name
        name: String,

        /// Agent kind: human, llm, tool, swarm
        #[arg(long)]
        kind: String,

        /// Model identifier (e.g. claude-opus-4-6)
        #[arg(long)]
        model: Option<String>,
    },

    /// List all agents
    List {
        /// Filter by kind
        #[arg(long)]
        kind: Option<String>,
    },

    /// Remove an agent
    Remove {
        /// Agent name to remove
        name: String,
    },

    /// Show agent details
    Info {
        /// Agent name
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let command = match cli.command {
        CliCommand::Init { path } => commands::Command::Init { path },
        CliCommand::Ping => commands::Command::Ping,
        CliCommand::Info => commands::Command::Info,
        CliCommand::Version => commands::Command::Version,
        CliCommand::Query { sql } => commands::Command::Query { sql },
        CliCommand::Checkout { file } => commands::Command::Checkout { file },
        CliCommand::Log { author, limit } => commands::Command::Log { author, limit },
        CliCommand::Commit { message } => commands::Command::Commit { message },
        CliCommand::Peer { action } => match action {
            PeerAction::Add {
                name,
                public_key,
                endpoint,
                connection,
            } => commands::Command::PeerAdd {
                name,
                public_key,
                endpoint,
                connection,
            },
            PeerAction::List => commands::Command::PeerList,
            PeerAction::Remove { name } => commands::Command::PeerRemove { name },
            PeerAction::Info { name } => commands::Command::PeerInfo { name },
        },
        CliCommand::Sync { peer } => commands::Command::Sync { peer },
        CliCommand::Find {
            pattern,
            kind,
            limit,
        } => commands::Command::Find {
            pattern,
            kind,
            limit,
        },
        CliCommand::Refs { symbol } => commands::Command::Refs { symbol },
        CliCommand::Tree { path } => commands::Command::Tree { path },
        CliCommand::Agent { action } => match action {
            AgentAction::Add { name, kind, model } => commands::Command::AgentAdd {
                name,
                kind,
                model,
            },
            AgentAction::List { kind } => commands::Command::AgentList { kind },
            AgentAction::Remove { name } => commands::Command::AgentRemove { name },
            AgentAction::Info { name } => commands::Command::AgentInfo { name },
        },
        CliCommand::Perspective {
            agent,
            context,
            min_weight,
        } => commands::Command::Perspective {
            agent,
            context_id: context,
            min_weight,
        },
        CliCommand::Consensus {
            context,
            min_agents,
            min_weight,
        } => commands::Command::Consensus {
            context_id: context,
            min_agents,
            min_weight,
        },
    };

    if let Err(e) = commands::run(command, &cli.profile, cli.db.as_deref(), &cli.format) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
