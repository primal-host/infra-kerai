mod case;
mod commands;
mod config;
mod db;
mod home;
mod lang;
mod output;

use std::collections::HashMap;
use std::env;

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
    /// Postgres AST operations — init, ping, query, find, tree, etc.
    Postgres {
        #[command(subcommand)]
        action: PostgresAction,
    },

    /// Sync CRDT operations with peers
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },

    /// Agent perspective views
    Perspective {
        #[command(subcommand)]
        action: PerspectiveAction,
    },

    /// Multi-agent consensus views
    Consensus {
        #[command(subcommand)]
        action: ConsensusAction,
    },

    /// Manage peer instances
    Peer {
        #[command(subcommand)]
        action: PeerAction,
    },

    /// Manage AI agents
    Agent {
        #[command(subcommand)]
        action: AgentAction,
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

    /// Knowledge marketplace — auctions, bids, Koi Pond
    Market {
        #[command(subcommand)]
        action: MarketAction,
    },

    /// Manage Koi wallets
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },

    /// Manage bounties
    Bounty {
        #[command(subcommand)]
        action: BountyAction,
    },

    /// Native currency — registration, signed transfers, supply, mining
    Currency {
        #[command(subcommand)]
        action: CurrencyAction,
    },

    /// Manage MicroGPT neural models
    Model {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand)]
enum PostgresAction {
    /// Set the global Postgres connection string
    Connect {
        /// Connection string (e.g. postgres://localhost/kerai)
        connection: String,
    },

    /// Import a project: create config and parse crate
    Import {
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

    /// Export source files reconstructed from AST
    Export {
        /// Export a single file by name
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
}

#[derive(Subcommand)]
enum SyncAction {
    /// Sync with a peer
    Run {
        /// Peer name to sync with
        peer: String,
    },
}

#[derive(Subcommand)]
enum PerspectiveAction {
    /// List an agent's perspectives
    List {
        /// Agent name
        agent: String,

        /// Filter by context node ID
        #[arg(long)]
        context: Option<String>,

        /// Minimum weight threshold
        #[arg(long)]
        min_weight: Option<f64>,
    },
}

#[derive(Subcommand)]
enum ConsensusAction {
    /// Show multi-agent consensus status
    Status {
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

        /// Starting price in Koi
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

    /// Browse the Koi Pond (open-sourced knowledge)
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
enum WalletAction {
    /// Create a new wallet
    Create {
        /// Wallet type: human, agent, or external
        #[arg(long)]
        r#type: String,

        /// Optional label
        #[arg(long)]
        label: Option<String>,
    },

    /// List wallets
    List {
        /// Filter by type
        #[arg(long)]
        r#type: Option<String>,
    },

    /// Show wallet balance
    Balance {
        /// Wallet ID (default: self instance wallet)
        wallet_id: Option<String>,
    },

    /// Transfer Koi between wallets
    Transfer {
        /// Source wallet ID
        #[arg(long)]
        from: String,

        /// Destination wallet ID
        #[arg(long)]
        to: String,

        /// Amount to transfer
        #[arg(long)]
        amount: i64,

        /// Transfer reason
        #[arg(long)]
        reason: Option<String>,
    },

    /// Show transaction history
    History {
        /// Wallet ID
        wallet_id: String,

        /// Maximum entries
        #[arg(long, default_value = "50")]
        limit: i32,
    },
}

#[derive(Subcommand)]
enum BountyAction {
    /// Create a bounty
    Create {
        /// Scope (ltree path)
        #[arg(long)]
        scope: String,

        /// Bounty description
        #[arg(long)]
        description: String,

        /// Reward in Koi
        #[arg(long)]
        reward: i64,

        /// Command to verify success
        #[arg(long)]
        success_command: Option<String>,

        /// Expiration timestamp (ISO 8601)
        #[arg(long)]
        expires: Option<String>,
    },

    /// List bounties
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,

        /// Filter by scope
        #[arg(long)]
        scope: Option<String>,
    },

    /// Show bounty details
    Show {
        /// Bounty ID
        bounty_id: String,
    },

    /// Claim a bounty
    Claim {
        /// Bounty ID
        bounty_id: String,

        /// Claimer wallet ID
        #[arg(long)]
        wallet: String,
    },

    /// Settle a claimed bounty (pay reward)
    Settle {
        /// Bounty ID
        bounty_id: String,
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

#[derive(Subcommand)]
enum ModelAction {
    /// Create a new neural model for an agent
    Create {
        /// Agent name
        #[arg(long)]
        agent: String,

        /// Embedding dimension (default 32)
        #[arg(long)]
        dim: Option<i32>,

        /// Number of attention heads (default 4)
        #[arg(long)]
        heads: Option<i32>,

        /// Number of transformer layers (default 1)
        #[arg(long)]
        layers: Option<i32>,

        /// Context length in nodes (default 16)
        #[arg(long)]
        context_len: Option<i32>,

        /// Scope (ltree path) to build vocabulary from
        #[arg(long)]
        scope: Option<String>,
    },

    /// Train a model on graph walks
    Train {
        /// Agent name
        #[arg(long)]
        agent: String,

        /// Walk type: tree, edge, perspective, random
        #[arg(long)]
        walks: Option<String>,

        /// Number of walk sequences
        #[arg(long)]
        sequences: Option<i32>,

        /// Number of training steps
        #[arg(long)]
        steps: Option<i32>,

        /// Learning rate
        #[arg(long)]
        lr: Option<f64>,

        /// Scope (ltree path) to generate walks within
        #[arg(long)]
        scope: Option<String>,

        /// Agent name for perspective-weighted walks
        #[arg(long)]
        perspective_agent: Option<String>,
    },

    /// Predict next nodes given a context
    Predict {
        /// Agent name
        #[arg(long)]
        agent: String,

        /// Comma-separated context node UUIDs
        #[arg(long)]
        context: String,

        /// Number of predictions to return
        #[arg(long)]
        top_k: Option<i32>,
    },

    /// Neural-enhanced search
    Search {
        /// Agent name
        #[arg(long)]
        agent: String,

        /// Search query text
        #[arg(long)]
        query: String,

        /// Number of results to return
        #[arg(long)]
        top_k: Option<i32>,
    },

    /// Ensemble prediction from multiple models
    Ensemble {
        /// Comma-separated agent names
        #[arg(long)]
        agents: String,

        /// Comma-separated context node UUIDs
        #[arg(long)]
        context: String,

        /// Number of predictions to return
        #[arg(long)]
        top_k: Option<i32>,
    },

    /// Show model info and training history
    Info {
        /// Agent name
        #[arg(long)]
        agent: String,
    },

    /// Delete a model's weights and vocabulary
    Delete {
        /// Agent name
        #[arg(long)]
        agent: String,
    },
}

#[derive(Subcommand)]
enum CurrencyAction {
    /// Register a wallet with a client-provided Ed25519 public key
    Register {
        /// Ed25519 public key (hex-encoded, 64 chars)
        #[arg(long)]
        pubkey: String,

        /// Wallet type: human, agent, or external
        #[arg(long)]
        r#type: String,

        /// Optional label
        #[arg(long)]
        label: Option<String>,
    },

    /// Signed transfer between wallets
    Transfer {
        /// Source wallet ID
        #[arg(long)]
        from: String,

        /// Destination wallet ID
        #[arg(long)]
        to: String,

        /// Amount to transfer
        #[arg(long)]
        amount: i64,

        /// Nonce (must be current wallet nonce + 1)
        #[arg(long)]
        nonce: i64,

        /// Ed25519 signature (hex-encoded)
        #[arg(long)]
        signature: String,

        /// Transfer reason
        #[arg(long)]
        reason: Option<String>,
    },

    /// Show total supply info
    Supply,

    /// Show wallet share of total supply
    Share {
        /// Wallet ID
        wallet_id: String,
    },

    /// List reward schedule
    Schedule,

    /// Create or update a reward schedule entry
    SetReward {
        /// Work type identifier
        #[arg(long)]
        work_type: String,

        /// Reward amount in Koi
        #[arg(long)]
        reward: i64,

        /// Enable or disable this reward
        #[arg(long)]
        enabled: Option<bool>,
    },
}

/// Known global flags that take a value argument.
const FLAGS_WITH_VALUE: &[&str] = &["--db", "--profile", "--format"];

/// Known subcommand names — if the first positional matches one, skip eval mode.
const SUBCOMMANDS: &[&str] = &[
    "postgres", "sync", "perspective", "consensus", "peer",
    "agent", "task", "swarm", "market", "wallet", "bounty",
    "currency", "model",
];

/// Notation switch tokens mapped to notation modes.
const NOTATION_SWITCHES: &[(&str, lang::Notation)] = &[
    (".pre", lang::Notation::Prefix),
    (".prefix", lang::Notation::Prefix),
    (".b", lang::Notation::Prefix),
    (".in", lang::Notation::Infix),
    (".infix", lang::Notation::Infix),
    (".c", lang::Notation::Infix),
    (".post", lang::Notation::Postfix),
    (".postfix", lang::Notation::Postfix),
    (".a", lang::Notation::Postfix),
];

/// Try to evaluate args as a calculator expression.
/// Runs on raw args (before `rewrite_args`) so notation switches like `.a` aren't mangled.
/// Returns `Some(result_string)` if evaluation happened, `None` to fall through to clap.
fn try_eval(args: &[String], aliases: &HashMap<String, String>) -> Option<String> {
    // Extract positional args (skip program name, skip flags and their values)
    let mut positionals = Vec::new();
    let mut i = 1; // skip program name
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with('-') {
            if FLAGS_WITH_VALUE.contains(&arg.as_str()) {
                i += 1; // skip the flag's value too
            }
            i += 1;
            continue;
        }
        positionals.push(arg.as_str());
        i += 1;
    }

    if positionals.is_empty() {
        return None;
    }

    // Check first positional for notation switch
    let (notation, expr_start) = if let Some(&(_, notation)) = NOTATION_SWITCHES
        .iter()
        .find(|&&(switch, _)| switch == positionals[0])
    {
        (notation, 1)
    } else if positionals[0].starts_with('.') {
        // Dot-prefixed but not a notation switch — probably a dot-notation subcommand
        return None;
    } else {
        // Check if first positional is a known subcommand
        if SUBCOMMANDS.contains(&positionals[0]) {
            return None;
        }
        // Check if it's an alias that expands to a known subcommand
        if let Some(expanded) = aliases.get(positionals[0]) {
            if SUBCOMMANDS.contains(&expanded.as_str()) {
                return None;
            }
        }
        (lang::Notation::Infix, 0)
    };

    let source: String = positionals[expr_start..].join(" ");
    if source.is_empty() {
        return None;
    }

    let expr = lang::parse_expr(&source, notation)?;
    let value = lang::eval::eval(&expr);
    Some(value.to_string())
}

/// Rewrites argv so that dot-namespaced commands become space-separated subcommands.
///
/// `kerai postgres.query "SQL"` → `["kerai", "postgres", "query", "SQL"]`
/// `kerai pg.query "SQL"`       → `["kerai", "postgres", "query", "SQL"]` (via alias)
/// `kerai postgres query "SQL"` → unchanged (already space-separated)
fn rewrite_args(
    args: impl Iterator<Item = String>,
    aliases: &HashMap<String, String>,
) -> Vec<String> {
    let args: Vec<String> = args.collect();
    let mut result = Vec::with_capacity(args.len() + 1);

    // Always keep program name
    if let Some(prog) = args.first() {
        result.push(prog.clone());
    }

    let mut i = 1;
    let mut found_positional = false;

    while i < args.len() {
        let arg = &args[i];

        // Skip flags and their values
        if arg.starts_with('-') {
            result.push(arg.clone());
            if FLAGS_WITH_VALUE.contains(&arg.as_str()) {
                i += 1;
                if i < args.len() {
                    result.push(args[i].clone());
                }
            }
            i += 1;
            continue;
        }

        // First positional arg: check for dot notation
        if !found_positional && arg.contains('.') {
            found_positional = true;
            if let Some((ns, cmd)) = arg.split_once('.') {
                let expanded = aliases.get(ns).map_or(ns, |v| v.as_str());
                result.push(expanded.to_string());
                result.push(cmd.to_string());
                i += 1;
                continue;
            }
        }

        // Non-dot positional or subsequent args: expand alias on first positional only
        if !found_positional {
            found_positional = true;
            let expanded = aliases.get(arg.as_str()).map_or(arg.clone(), |v| v.clone());
            result.push(expanded);
        } else {
            result.push(arg.clone());
        }
        i += 1;
    }

    result
}

fn main() {
    // Set up ~/.kerai/ and load aliases (non-fatal on failure)
    let aliases = match home::ensure_home_dir() {
        Ok(home) => {
            let _ = home::ensure_aliases_file(&home);
            let _ = home::ensure_kerai_file(&home);
            home::load_aliases(&home).unwrap_or_default()
        }
        Err(_) => HashMap::new(),
    };

    let raw_args: Vec<String> = env::args().collect();

    // Try calculator/eval mode on raw args (before rewrite mangles notation switches)
    if let Some(result) = try_eval(&raw_args, &aliases) {
        println!("{result}");
        return;
    }

    let args = rewrite_args(raw_args.into_iter(), &aliases);
    let cli = Cli::parse_from(args);

    let command = match cli.command {
        CliCommand::Postgres { action } => match action {
            PostgresAction::Connect { connection } => commands::Command::Connect { connection },
            PostgresAction::Import { path } => commands::Command::Import { path },
            PostgresAction::Ping => commands::Command::Ping,
            PostgresAction::Info => commands::Command::Info,
            PostgresAction::Version => commands::Command::Version,
            PostgresAction::Query { sql } => commands::Command::Query { sql },
            PostgresAction::Export { file } => commands::Command::Export { file },
            PostgresAction::Log { author, limit } => commands::Command::Log { author, limit },
            PostgresAction::Commit { message } => commands::Command::Commit { message },
            PostgresAction::Find {
                pattern,
                kind,
                limit,
            } => commands::Command::Find {
                pattern,
                kind,
                limit,
            },
            PostgresAction::Refs { symbol } => commands::Command::Refs { symbol },
            PostgresAction::Tree { path } => commands::Command::Tree { path },
        },
        CliCommand::Sync { action } => match action {
            SyncAction::Run { peer } => commands::Command::Sync { peer },
        },
        CliCommand::Perspective { action } => match action {
            PerspectiveAction::List {
                agent,
                context,
                min_weight,
            } => commands::Command::Perspective {
                agent,
                context_id: context,
                min_weight,
            },
        },
        CliCommand::Consensus { action } => match action {
            ConsensusAction::Status {
                context,
                min_agents,
                min_weight,
            } => commands::Command::Consensus {
                context_id: context,
                min_agents,
                min_weight,
            },
        },
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
        CliCommand::Wallet { action } => match action {
            WalletAction::Create { r#type, label } => commands::Command::WalletCreate {
                wallet_type: r#type,
                label,
            },
            WalletAction::List { r#type } => commands::Command::WalletList {
                wallet_type: r#type,
            },
            WalletAction::Balance { wallet_id } => commands::Command::WalletBalance { wallet_id },
            WalletAction::Transfer {
                from,
                to,
                amount,
                reason,
            } => commands::Command::WalletTransfer {
                from,
                to,
                amount,
                reason,
            },
            WalletAction::History { wallet_id, limit } => commands::Command::WalletHistory {
                wallet_id,
                limit,
            },
        },
        CliCommand::Bounty { action } => match action {
            BountyAction::Create {
                scope,
                description,
                reward,
                success_command,
                expires,
            } => commands::Command::BountyCreate {
                scope,
                description,
                reward,
                success_command,
                expires,
            },
            BountyAction::List { status, scope } => commands::Command::BountyList { status, scope },
            BountyAction::Show { bounty_id } => commands::Command::BountyShow { bounty_id },
            BountyAction::Claim { bounty_id, wallet } => commands::Command::BountyClaim {
                bounty_id,
                wallet_id: wallet,
            },
            BountyAction::Settle { bounty_id } => commands::Command::BountySettle { bounty_id },
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
        CliCommand::Model { action } => match action {
            ModelAction::Create {
                agent,
                dim,
                heads,
                layers,
                context_len,
                scope,
            } => commands::Command::ModelCreate {
                agent,
                dim,
                heads,
                layers,
                context_len,
                scope,
            },
            ModelAction::Train {
                agent,
                walks,
                sequences,
                steps,
                lr,
                scope,
                perspective_agent,
            } => commands::Command::ModelTrain {
                agent,
                walks,
                sequences,
                steps,
                lr,
                scope,
                perspective_agent,
            },
            ModelAction::Predict {
                agent,
                context,
                top_k,
            } => commands::Command::ModelPredict {
                agent,
                context,
                top_k,
            },
            ModelAction::Search {
                agent,
                query,
                top_k,
            } => commands::Command::ModelSearch {
                agent,
                query,
                top_k,
            },
            ModelAction::Ensemble {
                agents,
                context,
                top_k,
            } => commands::Command::ModelEnsemble {
                agents,
                context,
                top_k,
            },
            ModelAction::Info { agent } => commands::Command::ModelInfo { agent },
            ModelAction::Delete { agent } => commands::Command::ModelDelete { agent },
        },
        CliCommand::Currency { action } => match action {
            CurrencyAction::Register {
                pubkey,
                r#type,
                label,
            } => commands::Command::CurrencyRegister {
                pubkey,
                wallet_type: r#type,
                label,
            },
            CurrencyAction::Transfer {
                from,
                to,
                amount,
                nonce,
                signature,
                reason,
            } => commands::Command::CurrencyTransfer {
                from,
                to,
                amount,
                nonce,
                signature,
                reason,
            },
            CurrencyAction::Supply => commands::Command::CurrencySupply,
            CurrencyAction::Share { wallet_id } => {
                commands::Command::CurrencyShare { wallet_id }
            }
            CurrencyAction::Schedule => commands::Command::CurrencySchedule,
            CurrencyAction::SetReward {
                work_type,
                reward,
                enabled,
            } => commands::Command::CurrencySetReward {
                work_type,
                reward,
                enabled,
            },
        },
    };

    if let Err(e) = commands::run(command, &cli.profile, cli.db.as_deref(), &cli.format) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> impl Iterator<Item = String> {
        s.split_whitespace().map(String::from).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn dot_notation_expands() {
        let aliases = HashMap::new();
        let result = rewrite_args(args("kerai postgres.query SELECT"), &aliases);
        assert_eq!(result, vec!["kerai", "postgres", "query", "SELECT"]);
    }

    #[test]
    fn alias_expands_dot_notation() {
        let mut aliases = HashMap::new();
        aliases.insert("pg".to_string(), "postgres".to_string());
        let result = rewrite_args(args("kerai pg.query SELECT"), &aliases);
        assert_eq!(result, vec!["kerai", "postgres", "query", "SELECT"]);
    }

    #[test]
    fn space_form_unchanged() {
        let aliases = HashMap::new();
        let result = rewrite_args(args("kerai postgres query SELECT"), &aliases);
        assert_eq!(result, vec!["kerai", "postgres", "query", "SELECT"]);
    }

    #[test]
    fn alias_expands_space_form() {
        let mut aliases = HashMap::new();
        aliases.insert("pg".to_string(), "postgres".to_string());
        let result = rewrite_args(args("kerai pg query SELECT"), &aliases);
        assert_eq!(result, vec!["kerai", "postgres", "query", "SELECT"]);
    }

    #[test]
    fn flags_before_subcommand() {
        let aliases = HashMap::new();
        let result = rewrite_args(args("kerai --db mydb --format json postgres.ping"), &aliases);
        assert_eq!(result, vec!["kerai", "--db", "mydb", "--format", "json", "postgres", "ping"]);
    }

    #[test]
    fn no_dot_no_alias_passthrough() {
        let aliases = HashMap::new();
        let result = rewrite_args(args("kerai peer list"), &aliases);
        assert_eq!(result, vec!["kerai", "peer", "list"]);
    }

    // --- try_eval tests ---

    fn sv(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn no_aliases() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn eval_simple_addition() {
        assert_eq!(try_eval(&sv("kerai 3 + 4"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_precedence() {
        assert_eq!(try_eval(&sv("kerai 2 + 3 * 4"), &no_aliases()), Some("14".into()));
    }

    #[test]
    fn eval_postfix_switch() {
        assert_eq!(try_eval(&sv("kerai .post 3 4 +"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_postfix_chained() {
        assert_eq!(try_eval(&sv("kerai .a 1 2 3 * +"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_prefix_switch() {
        assert_eq!(try_eval(&sv("kerai .pre + 3 4"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_prefix_nested() {
        assert_eq!(try_eval(&sv("kerai .b + 1 * 2 3"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_hex_literal() {
        assert_eq!(try_eval(&sv("kerai 0xFF"), &no_aliases()), Some("255".into()));
    }

    #[test]
    fn eval_integer_division() {
        assert_eq!(try_eval(&sv("kerai 10 / 3"), &no_aliases()), Some("3".into()));
    }

    #[test]
    fn eval_float_division() {
        let result = try_eval(&sv("kerai 10.0 / 3"), &no_aliases()).unwrap();
        assert!(result.starts_with("3.333333333333333"));
    }

    #[test]
    fn eval_string_passthrough() {
        assert_eq!(try_eval(&sv("kerai hello"), &no_aliases()), Some("hello".into()));
    }

    #[test]
    fn eval_subcommand_falls_through() {
        assert_eq!(try_eval(&sv("kerai postgres ping"), &no_aliases()), None);
    }

    #[test]
    fn eval_alias_subcommand_falls_through() {
        let mut aliases = HashMap::new();
        aliases.insert("pg".to_string(), "postgres".to_string());
        assert_eq!(try_eval(&sv("kerai pg ping"), &aliases), None);
    }

    #[test]
    fn eval_no_positionals_falls_through() {
        assert_eq!(try_eval(&sv("kerai --db mydb"), &no_aliases()), None);
    }

    #[test]
    fn eval_skips_flags() {
        assert_eq!(try_eval(&sv("kerai --db mydb 3 + 4"), &no_aliases()), Some("7".into()));
    }

    #[test]
    fn eval_dot_subcommand_falls_through() {
        // Dot-prefixed but not a notation switch — should fall through
        assert_eq!(try_eval(&sv("kerai .unknown stuff"), &no_aliases()), None);
    }
}
