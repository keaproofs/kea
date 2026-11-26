//! Settings, all from the environment, each with a default that is safe to run.

use anyhow::{Context, Result, bail};
use kea_claims::model::Model;
use std::path::PathBuf;

/// How the API runs.
#[derive(Clone, Debug)]
pub struct Config {
    /// The SQLite file (`KEA_DB`).
    pub db: PathBuf,
    /// Where jobs write their projects (`KEA_WORK`).
    pub work: PathBuf,
    /// Address to listen on (`KEA_LISTEN`). Loopback by default: a tunnel or proxy publishes it.
    pub listen: String,
    /// The chain endpoint (`RH_RPC`).
    pub rpc_url: String,
    /// No new paid job starts once a UTC day's spend would pass this (`KEA_DAILY_BUDGET_USD`).
    pub daily_budget_usd: f64,
    /// What each open job is assumed to cost when admitting a new one (`KEA_JOB_RESERVE_USD`).
    pub job_reserve_usd: f64,
    /// Paid checks one requester may start per UTC day (`KEA_ASKS_PER_DAY`).
    pub asks_per_day: usize,
    /// The most jobs waiting or running (`KEA_MAX_QUEUE`).
    pub max_queue: usize,
    /// Browser origins allowed to call the API (`KEA_ORIGINS`, comma separated).
    pub origins: Vec<String>,
    /// Secret salt for hashing who asked; raw addresses are never stored (`KEA_SALT`).
    pub salt: String,
    /// Read the client address from `CF-Connecting-IP` (`KEA_TRUST_CF=1`), only when Cloudflare is
    /// the sole way in.
    pub trust_cloudflare: bool,
    /// The drafting and reviewing model (`KEA_MODEL`).
    pub model: Model,
    /// The most drafts per job (`KEA_ATTEMPTS`).
    pub attempts: u32,
    /// No new draft starts in a job past this spend (`KEA_JOB_BUDGET_USD`).
    pub job_budget_usd: f64,
    /// Minutes between checks that proved code is still the code at its address (`KEA_WATCH_MINUTES`).
    pub watch_minutes: u64,
    /// The public site, for links in feeds (`KEA_SITE`).
    pub site: String,
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn parsed<T: std::str::FromStr>(name: &str, default: T) -> Result<T> {
    var(name)
        .map_or(Ok(default), |value| value.parse().ok().with_context(|| format!("{name}={value:?} does not parse")))
}

impl Config {
    /// Reads the environment.
    ///
    /// # Errors
    /// When a value does not parse, or `KEA_SALT` is missing or short.
    pub fn from_env() -> Result<Self> {
        let salt = var("KEA_SALT").context("KEA_SALT is not set (32+ random characters)")?;
        if salt.len() < 32 {
            bail!("KEA_SALT is too short: use 32 or more random characters");
        }
        let model = var("KEA_MODEL").unwrap_or_else(|| "sonnet".into());
        Ok(Self {
            db: var("KEA_DB").map_or_else(|| PathBuf::from("/var/lib/kea/kea.db"), PathBuf::from),
            work: var("KEA_WORK").map_or_else(|| PathBuf::from("/var/lib/kea/work"), PathBuf::from),
            listen: var("KEA_LISTEN").unwrap_or_else(|| "127.0.0.1:8740".into()),
            rpc_url: var("RH_RPC").unwrap_or_else(|| kea_chain::ROBINHOOD_RPC.into()),
            daily_budget_usd: parsed("KEA_DAILY_BUDGET_USD", 5.0)?,
            job_reserve_usd: parsed("KEA_JOB_RESERVE_USD", 0.6)?,
            asks_per_day: parsed("KEA_ASKS_PER_DAY", 3)?,
            max_queue: parsed("KEA_MAX_QUEUE", 30)?,
            origins: var("KEA_ORIGINS")
                .unwrap_or_else(|| "http://localhost:5173".into())
                .split(',')
                .map(|origin| origin.trim().to_string())
                .filter(|origin| !origin.is_empty())
                .collect(),
            salt,
            trust_cloudflare: var("KEA_TRUST_CF").is_some_and(|value| value == "1"),
            model: Model::parse(&model)
                .with_context(|| format!("KEA_MODEL={model:?} must be opus, sonnet or haiku"))?,
            attempts: parsed("KEA_ATTEMPTS", 4)?,
            job_budget_usd: parsed("KEA_JOB_BUDGET_USD", 1.5)?,
            watch_minutes: parsed("KEA_WATCH_MINUTES", 15)?,
            site: var("KEA_SITE").unwrap_or_else(|| "https://keaproofs.io".into()),
        })
    }
}
