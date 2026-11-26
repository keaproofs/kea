//! `kea-api`: Kea over HTTP. People ask about a contract; one worker proves it; reports and badges
//! are served from the store.

mod badge;
mod config;
mod feed;
mod routes;
mod store;
mod view;
mod watcher;
mod worker;

use alloy_primitives::Address;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kea_chain::Rpc;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Notify;

#[derive(Parser)]
#[command(name = "kea-api", version, about = "Kea's HTTP API")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the API and run the worker (the default).
    Serve,
    /// List claims held for private disclosure: protections a counterexample broke, which go to
    /// the deployer before anyone else.
    Disclosures,
    /// Run one watcher pass now: record proved contracts whose code changed, and queue re-checks
    /// the day's budget allows.
    Watch,
    /// Load finished projects (`<dir>/<address>/result.json`, with `test/Proof.t.sol` and
    /// `review.json`) as reports.
    Import {
        /// The batch directory.
        dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::from_env()?;
    let store = store::Store::open(&config.db)?;
    let rpc = Rpc::new(config.rpc_url.clone());
    match Cli::parse().command {
        Some(Command::Import { dir }) => {
            let count = import(&store, &rpc, &dir).await?;
            println!("imported {count} reports");
            return Ok(());
        }
        Some(Command::Watch) => {
            let app = routes::App { store, config, rpc, wake: Notify::new(), admission: tokio::sync::Mutex::new(()) };
            println!("{} proved contracts changed", watcher::pass(&app).await?);
            return Ok(());
        }
        Some(Command::Disclosures) => {
            for report in store.recent(usize::MAX)? {
                for judged in view::withheld_claims(&report) {
                    let replay = judged.claim.replay.as_deref().unwrap_or("");
                    let holder = report.run.holders.as_ref().and_then(|h| h.get(replay)?.clone());
                    println!(
                        "{} {} {} [{}] {} | replay {} | holder {}",
                        report.address,
                        report.name,
                        judged.claim.id,
                        judged.verdict,
                        judged.claim.plain,
                        if replay.is_empty() { "-" } else { replay },
                        holder.map_or_else(
                            || "-".to_string(),
                            |h| format!(
                                "{} ({})",
                                h.address.as_deref().unwrap_or("?"),
                                h.kind.as_deref().unwrap_or("?")
                            )
                        ),
                    );
                }
            }
            return Ok(());
        }
        Some(Command::Serve) | None => {}
    }
    // Known limit: a job a restart cut short has its spend so far left out of the day's total.
    // Restarts are deploys, not traffic.
    let requeued = store.requeue_interrupted()?;
    if requeued > 0 {
        eprintln!("requeued {requeued} interrupted jobs");
    }
    let listen = config.listen.clone();
    let app = Arc::new(routes::App { store, config, rpc, wake: Notify::new(), admission: tokio::sync::Mutex::new(()) });
    tokio::spawn(worker::run(app.clone()));
    tokio::spawn(watcher::run(app.clone()));
    let listener = tokio::net::TcpListener::bind(&listen).await.with_context(|| format!("listening on {listen}"))?;
    eprintln!("kea-api on {listen}");
    axum::serve(listener, routes::router(app).into_make_service_with_connect_info::<SocketAddr>()).await?;
    Ok(())
}

/// Stores every finished project in `dir` as a report.
async fn import(store: &store::Store, rpc: &Rpc, dir: &Path) -> Result<usize> {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let project = entry?.path();
        let result = project.join("result.json");
        if !result.is_file() {
            continue;
        }
        let run: kea_core::ContractRun = serde_json::from_str(&std::fs::read_to_string(&result)?)
            .with_context(|| format!("parsing {}", result.display()))?;
        let address: Address = run.address.parse().with_context(|| format!("address in {}", result.display()))?;
        let proxy = kea_sources::proxy::resolve(rpc, address).await?;
        let runs = proxy.as_ref().map_or(address, |proxy| proxy.implementation);
        let code = rpc.code(runs).await?;
        let review: BTreeMap<String, serde_json::Value> = std::fs::read_to_string(project.join("review.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let checked_at = std::fs::metadata(&result)?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0));
        store.save_report(&store::Report {
            address: format!("{address:#x}"),
            name: run.name.rsplit(':').next().unwrap_or(&run.name).to_string(),
            implementation: proxy.map(|proxy| format!("{:#x}", proxy.implementation)),
            code_hash: format!("{:#x}", alloy_primitives::keccak256(&code)),
            test_source: std::fs::read_to_string(project.join("test/Proof.t.sol")).unwrap_or_default(),
            cost_usd: run.cost_usd,
            question: None,
            review,
            run,
            checked_at,
        })?;
        count += 1;
    }
    Ok(count)
}
