pub mod agent;
pub mod bounty;
pub mod checkout;
pub mod commit;
pub mod consensus_cmd;
pub mod currency;
pub mod find;
pub mod info;
pub mod init;
pub mod log;
pub mod market;
pub mod model;
pub mod peer;
pub mod perspective;
pub mod ping;
pub mod query;
pub mod refs;
pub mod swarm;
pub mod sync;
pub mod task;
pub mod tree;
pub mod version;
pub mod wallet;

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
    TaskCreate {
        description: String,
        success_command: String,
        scope: Option<String>,
        budget_ops: Option<i32>,
        budget_seconds: Option<i32>,
    },
    TaskList {
        status: Option<String>,
    },
    TaskShow {
        task_id: String,
    },
    SwarmLaunch {
        task_id: String,
        agents: i32,
        kind: String,
        model: Option<String>,
    },
    SwarmStatus {
        task_id: Option<String>,
    },
    SwarmStop {
        task_id: String,
    },
    SwarmLeaderboard {
        task_id: String,
    },
    SwarmProgress {
        task_id: String,
    },
    MarketCreate {
        attestation_id: String,
        starting_price: i64,
        floor_price: i64,
        price_decrement: i64,
        decrement_interval: i64,
        min_bidders: i32,
        open_delay_hours: i32,
    },
    MarketBid {
        auction_id: String,
        max_price: i64,
    },
    MarketSettle {
        auction_id: String,
    },
    MarketOpenSource {
        auction_id: String,
    },
    MarketBrowse {
        scope: Option<String>,
        max_price: Option<i64>,
        status: Option<String>,
    },
    MarketStatus {
        auction_id: String,
    },
    MarketBalance,
    MarketCommons {
        scope: Option<String>,
        since: Option<String>,
    },
    MarketStats,
    WalletCreate {
        wallet_type: String,
        label: Option<String>,
    },
    WalletList {
        wallet_type: Option<String>,
    },
    WalletBalance {
        wallet_id: Option<String>,
    },
    WalletTransfer {
        from: String,
        to: String,
        amount: i64,
        reason: Option<String>,
    },
    WalletHistory {
        wallet_id: String,
        limit: i32,
    },
    BountyCreate {
        scope: String,
        description: String,
        reward: i64,
        success_command: Option<String>,
        expires: Option<String>,
    },
    BountyList {
        status: Option<String>,
        scope: Option<String>,
    },
    BountyShow {
        bounty_id: String,
    },
    BountyClaim {
        bounty_id: String,
        wallet_id: String,
    },
    BountySettle {
        bounty_id: String,
    },
    CurrencyRegister {
        pubkey: String,
        wallet_type: String,
        label: Option<String>,
    },
    CurrencyTransfer {
        from: String,
        to: String,
        amount: i64,
        nonce: i64,
        signature: String,
        reason: Option<String>,
    },
    CurrencySupply,
    CurrencyShare {
        wallet_id: String,
    },
    CurrencySchedule,
    CurrencySetReward {
        work_type: String,
        reward: i64,
        enabled: Option<bool>,
    },
    ModelCreate {
        agent: String,
        dim: Option<i32>,
        heads: Option<i32>,
        layers: Option<i32>,
        context_len: Option<i32>,
        scope: Option<String>,
    },
    ModelTrain {
        agent: String,
        walks: Option<String>,
        sequences: Option<i32>,
        steps: Option<i32>,
        lr: Option<f64>,
        scope: Option<String>,
        perspective_agent: Option<String>,
    },
    ModelPredict {
        agent: String,
        context: String,
        top_k: Option<i32>,
    },
    ModelSearch {
        agent: String,
        query: String,
        top_k: Option<i32>,
    },
    ModelEnsemble {
        agents: String,
        context: String,
        top_k: Option<i32>,
    },
    ModelInfo {
        agent: String,
    },
    ModelDelete {
        agent: String,
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
        Command::TaskCreate {
            description,
            success_command,
            scope,
            budget_ops,
            budget_seconds,
        } => task::create(
            &mut client,
            &description,
            &success_command,
            scope.as_deref(),
            budget_ops,
            budget_seconds,
            format,
        ),
        Command::TaskList { status } => task::list(&mut client, status.as_deref(), format),
        Command::TaskShow { task_id } => task::show(&mut client, &task_id, format),
        Command::SwarmLaunch {
            task_id,
            agents,
            kind,
            model,
        } => swarm::launch(
            &mut client,
            &task_id,
            agents,
            &kind,
            model.as_deref(),
            format,
        ),
        Command::SwarmStatus { task_id } => {
            swarm::status(&mut client, task_id.as_deref(), format)
        }
        Command::SwarmStop { task_id } => swarm::stop(&mut client, &task_id),
        Command::SwarmLeaderboard { task_id } => {
            swarm::leaderboard(&mut client, &task_id, format)
        }
        Command::SwarmProgress { task_id } => {
            swarm::progress(&mut client, &task_id, format)
        }
        Command::MarketCreate {
            attestation_id,
            starting_price,
            floor_price,
            price_decrement,
            decrement_interval,
            min_bidders,
            open_delay_hours,
        } => market::create(
            &mut client,
            &attestation_id,
            starting_price,
            floor_price,
            price_decrement,
            decrement_interval,
            min_bidders,
            open_delay_hours,
            format,
        ),
        Command::MarketBid {
            auction_id,
            max_price,
        } => market::bid(&mut client, &auction_id, max_price, format),
        Command::MarketSettle { auction_id } => {
            market::settle(&mut client, &auction_id, format)
        }
        Command::MarketOpenSource { auction_id } => {
            market::open_source(&mut client, &auction_id)
        }
        Command::MarketBrowse {
            scope,
            max_price,
            status,
        } => market::browse(
            &mut client,
            scope.as_deref(),
            max_price,
            status.as_deref(),
            format,
        ),
        Command::MarketStatus { auction_id } => {
            market::status(&mut client, &auction_id, format)
        }
        Command::MarketBalance => market::balance(&mut client, format),
        Command::MarketCommons { scope, since } => {
            market::commons(&mut client, scope.as_deref(), since.as_deref(), format)
        }
        Command::MarketStats => market::stats(&mut client, format),
        Command::WalletCreate { wallet_type, label } => {
            wallet::create(&mut client, &wallet_type, label.as_deref(), format)
        }
        Command::WalletList { wallet_type } => {
            wallet::list(&mut client, wallet_type.as_deref(), format)
        }
        Command::WalletBalance { wallet_id } => {
            wallet::balance(&mut client, wallet_id.as_deref(), format)
        }
        Command::WalletTransfer {
            from,
            to,
            amount,
            reason,
        } => wallet::transfer(&mut client, &from, &to, amount, reason.as_deref(), format),
        Command::WalletHistory { wallet_id, limit } => {
            wallet::history(&mut client, &wallet_id, limit, format)
        }
        Command::BountyCreate {
            scope,
            description,
            reward,
            success_command,
            expires,
        } => bounty::create(
            &mut client,
            &scope,
            &description,
            reward,
            success_command.as_deref(),
            expires.as_deref(),
            format,
        ),
        Command::BountyList { status, scope } => {
            bounty::list(&mut client, status.as_deref(), scope.as_deref(), format)
        }
        Command::BountyShow { bounty_id } => bounty::show(&mut client, &bounty_id, format),
        Command::BountyClaim {
            bounty_id,
            wallet_id,
        } => bounty::claim(&mut client, &bounty_id, &wallet_id, format),
        Command::BountySettle { bounty_id } => bounty::settle(&mut client, &bounty_id, format),
        Command::CurrencyRegister {
            pubkey,
            wallet_type,
            label,
        } => currency::register(&mut client, &pubkey, &wallet_type, label.as_deref(), format),
        Command::CurrencyTransfer {
            from,
            to,
            amount,
            nonce,
            signature,
            reason,
        } => currency::transfer(
            &mut client,
            &from,
            &to,
            amount,
            nonce,
            &signature,
            reason.as_deref(),
            format,
        ),
        Command::CurrencySupply => currency::supply(&mut client, format),
        Command::CurrencyShare { wallet_id } => {
            currency::share(&mut client, &wallet_id, format)
        }
        Command::CurrencySchedule => currency::schedule(&mut client, format),
        Command::CurrencySetReward {
            work_type,
            reward,
            enabled,
        } => currency::set_reward(&mut client, &work_type, reward, enabled, format),
        Command::ModelCreate {
            agent,
            dim,
            heads,
            layers,
            context_len,
            scope,
        } => model::create(
            &mut client,
            &agent,
            dim,
            heads,
            layers,
            context_len,
            scope.as_deref(),
            format,
        ),
        Command::ModelTrain {
            agent,
            walks,
            sequences,
            steps,
            lr,
            scope,
            perspective_agent,
        } => model::train(
            &mut client,
            &agent,
            walks.as_deref(),
            sequences,
            steps,
            lr,
            scope.as_deref(),
            perspective_agent.as_deref(),
            format,
        ),
        Command::ModelPredict {
            agent,
            context,
            top_k,
        } => model::predict(&mut client, &agent, &context, top_k, format),
        Command::ModelSearch {
            agent,
            query,
            top_k,
        } => model::search(&mut client, &agent, &query, top_k, format),
        Command::ModelEnsemble {
            agents,
            context,
            top_k,
        } => model::ensemble(&mut client, &agents, &context, top_k, format),
        Command::ModelInfo { agent } => model::info(&mut client, &agent, format),
        Command::ModelDelete { agent } => model::delete(&mut client, &agent, format),
    }
}
