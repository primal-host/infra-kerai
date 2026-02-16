pub mod agent;
pub mod checkout;
pub mod commit;
pub mod consensus_cmd;
pub mod find;
pub mod info;
pub mod init;
pub mod log;
pub mod peer;
pub mod perspective;
pub mod ping;
pub mod query;
pub mod refs;
pub mod sync;
pub mod tree;
pub mod version;

use crate::config;
use crate::db;
use crate::output::OutputFormat;

pub enum Command {
    Init {
        path: Option<String>,
    },
    Ping,
    Info,
    Version,
    Query {
        sql: String,
    },
    Checkout {
        file: Option<String>,
    },
    Log {
        author: Option<String>,
        limit: i64,
    },
    Commit {
        message: Option<String>,
    },
    PeerAdd {
        name: String,
        public_key: String,
        endpoint: Option<String>,
        connection: Option<String>,
    },
    PeerList,
    PeerRemove {
        name: String,
    },
    PeerInfo {
        name: String,
    },
    Sync {
        peer: String,
    },
    Find {
        pattern: String,
        kind: Option<String>,
        limit: Option<i32>,
    },
    Refs {
        symbol: String,
    },
    Tree {
        path: Option<String>,
    },
    AgentAdd {
        name: String,
        kind: String,
        model: Option<String>,
    },
    AgentList {
        kind: Option<String>,
    },
    AgentRemove {
        name: String,
    },
    AgentInfo {
        name: String,
    },
    Perspective {
        agent: String,
        context_id: Option<String>,
        min_weight: Option<f64>,
    },
    Consensus {
        context_id: Option<String>,
        min_agents: Option<i32>,
        min_weight: Option<f64>,
    },
}

pub fn run(
    command: Command,
    profile_name: &str,
    db_override: Option<&str>,
    format: &OutputFormat,
) -> Result<(), String> {
    let profile = config::load_config(profile_name);

    // Determine the connection string for init's config file
    let conn_str = db_override
        .or(profile.connection.as_deref())
        .unwrap_or("postgres://localhost/kerai")
        .to_string();

    let mut client = db::connect(&profile, db_override)?;

    match command {
        Command::Init { path } => init::run(&mut client, path.as_deref(), &conn_str, format),
        Command::Ping => ping::run(&mut client),
        Command::Info => info::run(&mut client, format),
        Command::Version => version::run(&mut client, format),
        Command::Query { sql } => query::run(&mut client, &sql, format),
        Command::Checkout { file } => checkout::run(&mut client, file.as_deref()),
        Command::Log { author, limit } => log::run(&mut client, author.as_deref(), limit, format),
        Command::Commit { message } => commit::run(&mut client, message.as_deref()),
        Command::PeerAdd {
            name,
            public_key,
            endpoint,
            connection,
        } => peer::add(
            &mut client,
            &name,
            &public_key,
            endpoint.as_deref(),
            connection.as_deref(),
            format,
        ),
        Command::PeerList => peer::list(&mut client, format),
        Command::PeerRemove { name } => peer::remove(&mut client, &name),
        Command::PeerInfo { name } => peer::info(&mut client, &name, format),
        Command::Sync { peer } => sync::run(&mut client, &peer),
        Command::Find {
            pattern,
            kind,
            limit,
        } => find::run(&mut client, &pattern, kind.as_deref(), limit, format),
        Command::Refs { symbol } => refs::run(&mut client, &symbol, format),
        Command::Tree { path } => tree::run(&mut client, path.as_deref(), format),
        Command::AgentAdd { name, kind, model } => {
            agent::add(&mut client, &name, &kind, model.as_deref(), format)
        }
        Command::AgentList { kind } => agent::list(&mut client, kind.as_deref(), format),
        Command::AgentRemove { name } => agent::remove(&mut client, &name),
        Command::AgentInfo { name } => agent::info(&mut client, &name, format),
        Command::Perspective {
            agent,
            context_id,
            min_weight,
        } => perspective::run(
            &mut client,
            &agent,
            context_id.as_deref(),
            min_weight,
            format,
        ),
        Command::Consensus {
            context_id,
            min_agents,
            min_weight,
        } => consensus_cmd::run(
            &mut client,
            context_id.as_deref(),
            min_agents,
            min_weight,
            format,
        ),
    }
}
