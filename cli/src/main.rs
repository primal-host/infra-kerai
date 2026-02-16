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

    /// Manage swarm tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },

    /// Manage agent swarms
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },

    /// Knowledge marketplace — auctions, bids, commons
    Market {
        #[command(subcommand)]
        action: MarketAction,
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
enum TaskAction {
    /// Create a new task
    Create {
        /// Task description
        description: String,

        /// Command to verify success
        #[arg(long)]
        success_command: String,

        /// Scope node ID (optional)
        #[arg(long)]
        scope: Option<String>,

        /// Operation budget limit
        #[arg(long)]
        budget_ops: Option<i32>,

        /// Time budget in seconds
        #[arg(long)]
        budget_seconds: Option<i32>,
    },

    /// List tasks
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
    },

    /// Show task details
    Show {
        /// Task ID
        task_id: String,
    },
}

#[derive(Subcommand)]
enum SwarmAction {
    /// Launch a swarm for a task
    Launch {
        /// Task ID
        task_id: String,

        /// Number of agents
        #[arg(long, default_value = "3")]
        agents: i32,

        /// Agent kind: llm, tool, human
        #[arg(long)]
        kind: String,

        /// Model identifier (e.g. claude-opus-4-6)
        #[arg(long)]
        model: Option<String>,
    },

    /// Show swarm status
    Status {
        /// Task ID (omit for all tasks)
        task_id: Option<String>,
    },

    /// Stop a running swarm
    Stop {
        /// Task ID
        task_id: String,
    },

    /// Show per-agent leaderboard
    Leaderboard {
        /// Task ID
        task_id: String,
    },

    /// Show pass rate over time
    Progress {
        /// Task ID
        task_id: String,
    },
}

#[derive(Subcommand)]
enum MarketAction {
    /// Create a Dutch auction for an attestation
    Create {
        /// Attestation ID
        attestation_id: String,

        /// Starting price in kōi
        #[arg(long)]
        starting_price: i64,

        /// Floor price (0 = always goes open)
        #[arg(long, default_value = "0")]
        floor_price: i64,

        /// Price decrease per interval
        #[arg(long)]
        price_decrement: i64,

        /// Interval between price drops (seconds)
        #[arg(long)]
        decrement_interval: i64,

        /// Minimum bidders to trigger settlement
        #[arg(long, default_value = "1")]
        min_bidders: i32,

        /// Hours after settlement before open-sourcing
        #[arg(long, default_value = "24")]
        open_delay_hours: i32,
    },

    /// Place a bid on an auction
    Bid {
        /// Auction ID
        auction_id: String,

        /// Maximum price willing to pay
        #[arg(long)]
        max_price: i64,
    },

    /// Settle an auction (pay winning bidders)
    Settle {
        /// Auction ID
        auction_id: String,
    },

    /// Open-source a settled auction
    OpenSource {
        /// Auction ID
        auction_id: String,
    },

    /// Browse auctions
    Browse {
        /// Filter by scope (ltree path)
        #[arg(long)]
        scope: Option<String>,

        /// Maximum price filter
        #[arg(long)]
        max_price: Option<i64>,

        /// Filter by status (active, settled, open_sourced)
        #[arg(long)]
        status: Option<String>,
    },

    /// Show auction details and bids
    Status {
        /// Auction ID
        auction_id: String,
    },

    /// Show marketplace earnings and spending
    Balance,

    /// Browse open-sourced knowledge
    Commons {
        /// Filter by scope (ltree path)
        #[arg(long)]
        scope: Option<String>,

        /// Filter by date (ISO 8601)
        #[arg(long)]
        since: Option<String>,
    },

    /// Show marketplace statistics
    Stats,
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
        CliCommand::Task { action } => match action {
            TaskAction::Create {
                description,
                success_command,
                scope,
                budget_ops,
                budget_seconds,
            } => commands::Command::TaskCreate {
                description,
                success_command,
                scope,
                budget_ops,
                budget_seconds,
            },
            TaskAction::List { status } => commands::Command::TaskList { status },
            TaskAction::Show { task_id } => commands::Command::TaskShow { task_id },
        },
        CliCommand::Swarm { action } => match action {
            SwarmAction::Launch {
                task_id,
                agents,
                kind,
                model,
            } => commands::Command::SwarmLaunch {
                task_id,
                agents,
                kind,
                model,
            },
            SwarmAction::Status { task_id } => commands::Command::SwarmStatus { task_id },
            SwarmAction::Stop { task_id } => commands::Command::SwarmStop { task_id },
            SwarmAction::Leaderboard { task_id } => {
                commands::Command::SwarmLeaderboard { task_id }
            }
            SwarmAction::Progress { task_id } => commands::Command::SwarmProgress { task_id },
        },
        CliCommand::Market { action } => match action {
            MarketAction::Create {
                attestation_id,
                starting_price,
                floor_price,
                price_decrement,
                decrement_interval,
                min_bidders,
                open_delay_hours,
            } => commands::Command::MarketCreate {
                attestation_id,
                starting_price,
                floor_price,
                price_decrement,
                decrement_interval,
                min_bidders,
                open_delay_hours,
            },
            MarketAction::Bid {
                auction_id,
                max_price,
            } => commands::Command::MarketBid {
                auction_id,
                max_price,
            },
            MarketAction::Settle { auction_id } => {
                commands::Command::MarketSettle { auction_id }
            }
            MarketAction::OpenSource { auction_id } => {
                commands::Command::MarketOpenSource { auction_id }
            }
            MarketAction::Browse {
                scope,
                max_price,
                status,
            } => commands::Command::MarketBrowse {
                scope,
                max_price,
                status,
            },
            MarketAction::Status { auction_id } => {
                commands::Command::MarketStatus { auction_id }
            }
            MarketAction::Balance => commands::Command::MarketBalance,
            MarketAction::Commons { scope, since } => {
                commands::Command::MarketCommons { scope, since }
            }
            MarketAction::Stats => commands::Command::MarketStats,
        },
    };

    if let Err(e) = commands::run(command, &cli.profile, cli.db.as_deref(), &cli.format) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
