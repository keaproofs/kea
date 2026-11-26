//! `kea`: ask contracts questions and get proofs back.

mod batch;

use kea_engine::{ask, prove, review};

use alloy_primitives::Address;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kea_core::run::{MAX_PROVE_SECONDS, MIN_PROVEN};
use kea_core::{Claim, ContractRun, Reviews, Scored};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Minimum passing contracts for a batch to pass.
const NEED_PASSING: usize = 20;
/// Minimum replayed findings for a batch to pass.
const NEED_FINDINGS: usize = 3;

#[derive(Parser)]
#[command(name = "kea", version, about = "Proofs about deployed contracts")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Score a batch of contract runs (`<dir>/<address>/result.json`) against the pass mark.
    Score {
        /// The batch directory.
        dir: PathBuf,
        /// Print every claim with its verdict.
        #[arg(long)]
        claims: bool,
        /// Manual review file; defaults to `reviewed.json` next to the batch directory.
        #[arg(long)]
        reviews: Option<PathBuf>,
    },
    /// Prove a drafted test file against a deployed contract: fetch its verified source, build,
    /// prove every check, replay counterexamples on a fork, and score the claims.
    Prove {
        /// The deployed address (a proxy is followed to its implementation).
        address: Address,
        /// The claims: a JSON array, or a result file with a `claims` field.
        #[arg(long)]
        claims: PathBuf,
        /// The drafted Solidity test file.
        #[arg(long)]
        test: PathBuf,
        /// The directory that receives the project.
        #[arg(long, default_value = "work")]
        work: PathBuf,
        /// The chain endpoint.
        #[arg(long, default_value = kea_chain::ROBINHOOD_RPC)]
        rpc: String,
    },
    /// Ask a deployed contract a question: draft claims and checks with the Claude API, prove
    /// them, and redraft from the evidence. Spends API credit; reads `ANTHROPIC_API_KEY`.
    Ask {
        /// The deployed address (a proxy is followed to its implementation).
        address: Address,
        /// A question in plain words, answered first ("can the owner stop me from selling?").
        #[arg(long)]
        question: Option<String>,
        /// The drafting model: sonnet, opus or haiku. Sonnet won the first model eval: on ten contracts it
        /// tied Opus on contracts passing after a hand check, with more honest proofs, at half the cost.
        #[arg(long, default_value = "sonnet")]
        model: String,
        /// The model that reviews proven claims; the drafting model when not given.
        #[arg(long)]
        review_model: Option<String>,
        /// The most drafts to try.
        #[arg(long, default_value_t = 4)]
        attempts: u32,
        /// No new draft starts once this many US dollars are spent.
        #[arg(long, default_value_t = 2.0)]
        budget: f64,
        /// The directory that receives the project.
        #[arg(long, default_value = "work")]
        work: PathBuf,
        /// The chain endpoint.
        #[arg(long, default_value = kea_chain::ROBINHOOD_RPC)]
        rpc: String,
    },
    /// Review the proven claims of every project in a batch (`<dir>/<address>/result.json`):
    /// does each claim's wording say only what its checks proved? Writes each project's
    /// `review.json`, which `score` applies. Spends API credit; reads `ANTHROPIC_API_KEY`.
    Review {
        /// The batch directory.
        dir: PathBuf,
        /// The reviewing model: opus, sonnet or haiku.
        #[arg(long, default_value = "sonnet")]
        model: String,
        /// A hand-check review file to compare against; its verdicts mark claims that failed.
        #[arg(long)]
        labels: Option<PathBuf>,
        /// The chain endpoint.
        #[arg(long, default_value = kea_chain::ROBINHOOD_RPC)]
        rpc: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Score { dir, claims, reviews } => {
            let reviews_path = reviews.unwrap_or_else(|| dir.parent().unwrap_or(Path::new(".")).join("reviewed.json"));
            print!("{}", score_batch(&dir, &load_reviews(&reviews_path)?, claims)?);
        }
        Command::Prove { address, claims, test, work, rpc } => {
            let test_source = fs::read_to_string(&test).with_context(|| format!("reading {}", test.display()))?;
            let claims = load_claims(&claims)?;
            let request = prove::Request { address, claims, test_source: test_source.clone(), work, rpc_url: rpc };
            let run = prove::prove(request).await?;
            print!("{}", one(&run.score(&test_source, &Reviews::default())));
        }
        Command::Ask { address, question, model, review_model, attempts, budget, work, rpc } => {
            let review_model = parse_model(review_model.as_deref().unwrap_or(&model))?;
            let request = ask::Request {
                address,
                question,
                model: parse_model(&model)?,
                review_model,
                max_attempts: attempts,
                budget_usd: budget,
                work,
                rpc_url: rpc,
                meter: ask::Meter::default(),
            };
            let answer = ask::ask(request).await?;
            print!("{}", one(&answer.run.score(&answer.test_source, &answer.review.reviews())));
        }
        Command::Review { dir, model, labels, rpc } => {
            let labels = labels.map(|path| load_reviews(&path)).transpose()?;
            print!("{}", batch::review_batch(&dir, parse_model(&model)?, labels.as_ref(), &rpc).await?);
        }
    }
    Ok(())
}

fn parse_model(name: &str) -> Result<kea_claims::model::Model> {
    kea_claims::model::Model::parse(name).with_context(|| format!("model {name:?} must be opus, sonnet or haiku"))
}

/// Claims from a JSON array, or from the `claims` field of a result file.
fn load_claims(path: &Path) -> Result<Vec<Claim>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let claims = value.get("claims").cloned().unwrap_or(value);
    serde_json::from_value(claims).with_context(|| format!("reading claims from {}", path.display()))
}

fn load_reviews(path: &Path) -> Result<Reviews> {
    if !path.exists() {
        return Ok(Reviews::default());
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Reviews::from_json(&file)?)
}

fn score_batch(dir: &Path, reviews: &Reviews, with_claims: bool) -> Result<String> {
    let mut runs: Vec<PathBuf> = fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path().join("result.json")))
        .filter(|path| path.is_file())
        .collect();
    runs.sort();
    let mut scored = Vec::with_capacity(runs.len());
    for path in &runs {
        let run: ContractRun =
            serde_json::from_str(&fs::read_to_string(path)?).with_context(|| format!("parsing {}", path.display()))?;
        let test = path.with_file_name("test").join("Proof.t.sol");
        let source = fs::read_to_string(&test).unwrap_or_default();
        // A hand review decides over the model's review of the same claim.
        let reviews = load_reviews(&path.with_file_name(review::REVIEW_FILE))?.under(reviews);
        scored.push(run.score(&source, &reviews));
    }
    Ok(report(&scored, with_claims))
}

/// One contract's report: its line and claims, and whether it meets the pass mark, without the
/// batch tally, whose targets mean nothing for a single contract.
fn one(scored: &Scored) -> String {
    let full = report(std::slice::from_ref(scored), true);
    let mut out = full.split("\n\n").next().unwrap_or(&full).to_string();
    let verdict = if scored.passes() { "passes" } else { "does not pass" };
    let _ = writeln!(out, "\n{verdict} the bar of {MIN_PROVEN} proven within {} min", MAX_PROVE_SECONDS / 60);
    out
}

fn report(scored: &[Scored], with_claims: bool) -> String {
    let mut out = String::new();
    for s in scored {
        let name: String = s.name.chars().take(30).collect();
        let _ = writeln!(
            out,
            "{:6} {:30} drafts {} built {} prove {:4}s proven {} findings {} dropped {} unreplayed {} weak {} unknown {} ${:.2}",
            s.kind.as_deref().unwrap_or(""),
            name,
            s.drafts,
            if s.built { "Y" } else { "n" },
            s.prove_seconds,
            s.proven(),
            s.findings(),
            s.dropped(),
            s.unreplayed(),
            s.weak(),
            s.unknown(),
            s.cost_usd,
        );
        if with_claims {
            for judged in &s.claims {
                let scope = judged.claim.scope.as_deref().filter(|scope| !scope.is_empty());
                let _ = writeln!(
                    out,
                    "      {:18} {:16} {}{}",
                    judged.verdict.name(),
                    judged.claim.id,
                    judged.claim.plain,
                    scope.map(|scope| format!("  [scope: {scope}]")).unwrap_or_default(),
                );
            }
        }
    }
    let passing = scored.iter().filter(|s| s.passes()).count();
    let _ = writeln!(
        out,
        "\n{} subjects | >={MIN_PROVEN} proven within {} min: {passing} (need {NEED_PASSING}) | replayed findings: {} (need {NEED_FINDINGS}) | \
         dropped findings: {} | unreplayed counterexamples held back: {} | AI cost (API-equivalent): ${:.2}",
        scored.len(),
        MAX_PROVE_SECONDS / 60,
        scored.iter().map(Scored::findings).sum::<usize>(),
        scored.iter().map(Scored::dropped).sum::<usize>(),
        scored.iter().map(Scored::unreplayed).sum::<usize>(),
        scored.iter().map(|s| s.cost_usd).sum::<f64>(),
    );
    out
}
