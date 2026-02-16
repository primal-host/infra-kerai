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
    };

    if let Err(e) = commands::run(command, &cli.profile, cli.db.as_deref(), &cli.format) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
