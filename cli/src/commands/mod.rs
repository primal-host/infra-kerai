pub mod checkout;
pub mod commit;
pub mod info;
pub mod init;
pub mod log;
pub mod peer;
pub mod ping;
pub mod query;
pub mod sync;
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
    }
}
