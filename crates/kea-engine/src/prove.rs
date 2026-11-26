//! One contract end to end: fetch, write out, build, prove, replay, resolve holders.

use alloy_primitives::Address;
use anyhow::{Context, Result};
use kea_chain::{ROBINHOOD_CHAIN_ID, Rpc};
use kea_core::{Claim, ContractRun};
use kea_prover::process::Limits;
use kea_prover::{checksum, forge, halmos, resolve_holders};
use kea_sources::Subject;
use kea_sources::project::{self, Libraries};
use kea_sources::verified::Explorer;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUILD_LIMITS: Limits = Limits { wall: Duration::from_secs(900), grace: Duration::from_secs(30) };

/// What the tool steps leave in the project, cleared before each attempt.
pub const STEP_LOGS: [&str; 7] =
    ["build.log", "build.err.log", "halmos.log", "halmos.err.log", "halmos.json", "replay.log", "replay.json"];

/// A subject written out as a project, ready for attempts.
pub struct Workspace {
    /// What is being proved.
    pub subject: Subject,
    /// The project directory.
    pub dir: PathBuf,
    /// The chain client.
    pub rpc: Rpc,
    rpc_url: String,
}

/// Fetches the subject at `address` and writes it out under `work`, replacing any earlier project.
///
/// # Errors
/// When the subject cannot be fetched or written.
pub async fn prepare(address: Address, work: &Path, rpc_url: &str) -> Result<Workspace> {
    let rpc = Rpc::new(rpc_url.to_string());
    let explorer = Explorer::new(ROBINHOOD_CHAIN_ID);
    let subject = kea_sources::fetch(&rpc, &explorer, address).await.context("fetching the subject")?;
    let dir = work.join(format!("{address:#x}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).with_context(|| format!("clearing {}", dir.display()))?;
    }
    std::fs::create_dir_all(dir.join("test"))?;
    project::write(&dir, &subject.source, &Libraries::default())?;
    Ok(Workspace { subject, dir, rpc, rpc_url: rpc_url.to_string() })
}

impl Workspace {
    /// Builds, proves and replays one test file, and writes `result.json`.
    ///
    /// # Errors
    /// When a tool cannot be started. A build that fails, a check that times out or a replay that
    /// reverts is a result, not an error.
    pub async fn attempt(&self, claims: Vec<Claim>, test_source: &str) -> Result<ContractRun> {
        // A step this attempt skips must not leave an earlier attempt's log to pass for its own.
        for log in STEP_LOGS {
            let _ = std::fs::remove_file(self.dir.join(log));
        }
        self.write_test(test_source)?;
        let mut status = BTreeMap::new();
        let mut set = |key: &str, value: String| status.insert(key.to_string(), Value::String(value));
        let built = forge::build(&self.dir, BUILD_LIMITS).await?;
        set("build", exit_word(built.exit_code));
        let mut run = self.blank(claims);
        if built.succeeded() {
            let proved = halmos::prove(&self.dir, &halmos::Options::default()).await?;
            set("prove", exit_word(proved.run.exit_code));
            set("prove_s", proved.run.wall.as_secs().to_string());
            set("prove_cpu", whole_seconds(proved.run.cpu_seconds));
            run.halmos = proved.checks;
            if forge::has_replays(test_source) {
                let (replayed, replay_run) = forge::replay(&self.dir, &self.rpc_url, forge::REPLAY_LIMITS).await?;
                set("replay", exit_word(replay_run.exit_code));
                run.holders = Some(resolve_holders(&self.rpc, &replayed.holders).await);
                run.replay = replayed.runs;
            } else {
                set("replay", "none".into());
            }
        }
        run.status = Some(status);
        self.save(&run)?;
        Ok(run)
    }

    /// A run record for this subject with nothing run yet.
    pub fn blank(&self, claims: Vec<Claim>) -> ContractRun {
        ContractRun {
            address: self.subject.address.to_checksum(None),
            name: self.subject.source.fully_qualified_name.clone(),
            kind: None,
            claims,
            halmos: BTreeMap::new(),
            replay: BTreeMap::new(),
            holders: Some(BTreeMap::new()),
            status: None,
            attempts: vec![],
            cost_usd: 0.0,
        }
    }

    /// Writes the test file the project builds, with address literals checksummed as solc requires.
    ///
    /// # Errors
    /// When the file cannot be written.
    pub fn write_test(&self, test_source: &str) -> Result<()> {
        std::fs::write(self.dir.join("test/Proof.t.sol"), checksum::checksum_addresses(test_source))?;
        Ok(())
    }

    /// Writes `result.json`.
    ///
    /// # Errors
    /// When the file cannot be written.
    pub fn save(&self, run: &ContractRun) -> Result<()> {
        std::fs::write(self.dir.join("result.json"), serde_json::to_string_pretty(run)?)?;
        Ok(())
    }

    /// A log the last attempt left in the project, or an empty string.
    pub fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }
}

/// What to prove: a test file already written.
pub struct Request {
    /// The deployed address.
    pub address: Address,
    /// The claims the test file decides.
    pub claims: Vec<Claim>,
    /// The test file.
    pub test_source: String,
    /// The directory that receives one project per address.
    pub work: PathBuf,
    /// The chain endpoint for proxy reads, replays and holders.
    pub rpc_url: String,
}

/// Proves a given test file against a deployed contract.
///
/// # Errors
/// See [`prepare`] and [`Workspace::attempt`].
pub async fn prove(request: Request) -> Result<ContractRun> {
    let workspace = prepare(request.address, &request.work, &request.rpc_url).await?;
    workspace.attempt(request.claims, &request.test_source).await
}

fn exit_word(code: Option<i32>) -> String {
    code.map_or_else(|| "timeout".into(), |code| code.to_string())
}

/// Whole seconds, rounded down, as the pass mark counts them.
fn whole_seconds(seconds: f64) -> String {
    format!("{:.0}", seconds.max(0.0).floor())
}
