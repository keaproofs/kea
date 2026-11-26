//! Building with forge, and replaying counterexamples on a fork of the live chain.

use crate::process::{self, Finished, Limits, Run};
use alloy_primitives::Address;
use kea_core::run::ReplayRun;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

/// A replay names the account holding the power it used with `log_named_address("holder", …)`.
static HOLDER_LOG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"holder: (0x[0-9a-fA-F]{40})").expect("static pattern"));

/// Compiles the project from scratch.
///
/// # Errors
/// When forge cannot be started.
pub async fn build(project: &Path, limits: Limits) -> std::io::Result<Finished> {
    process::run(Run {
        program: "forge",
        args: vec!["build".into(), "--force".into()],
        dir: project,
        env: vec![],
        stdout: project.join("build.log"),
        stderr: project.join("build.err.log"),
        limits,
    })
    .await
}

/// Whether a test file has any replays to run.
pub fn has_replays(test_source: &str) -> bool {
    test_source.contains("function test_replay_")
}

/// What the replays showed.
#[derive(Clone, Debug, Default)]
pub struct Replayed {
    /// Results by replay name.
    pub runs: BTreeMap<String, ReplayRun>,
    /// The holder each replay logged, by replay name.
    pub holders: BTreeMap<String, Address>,
}

/// The default cap for one contract's replays.
pub const REPLAY_LIMITS: Limits = Limits { wall: Duration::from_secs(300), grace: Duration::from_secs(30) };

/// Runs the `test_replay_` functions against a fork of the chain at `rpc_url`.
///
/// # Errors
/// When forge cannot be started.
pub async fn replay(project: &Path, rpc_url: &str, limits: Limits) -> std::io::Result<(Replayed, Finished)> {
    let output = project.join("replay.json");
    let run = process::run(Run {
        program: "forge",
        args: ["test", "--match-contract", "ProofTest", "--match-test", "test_replay", "--json", "-vv"]
            .map(String::from)
            .to_vec(),
        dir: project,
        env: vec![("RH_RPC".into(), rpc_url.into())],
        stdout: output.clone(),
        stderr: project.join("replay.log"),
        limits,
    })
    .await?;
    let replayed = std::fs::read_to_string(&output)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|json| parse(&json))
        .unwrap_or_default();
    Ok((replayed, run))
}

/// Replay results and logged holders from `forge test --json` output (suite → `test_results`).
pub fn parse(json: &Value) -> Replayed {
    let mut replayed = Replayed::default();
    let suites = json.as_object().into_iter().flat_map(|suites| suites.values());
    for tests in suites.filter_map(|suite| suite.get("test_results").and_then(Value::as_object)) {
        for (signature, test) in tests {
            let name = signature.split('(').next().unwrap_or(signature).to_string();
            let reason = test.get("reason").and_then(Value::as_str).map(|r| r.chars().take(300).collect());
            replayed.runs.insert(
                name.clone(),
                ReplayRun {
                    status: test.get("status").and_then(Value::as_str).map(str::to_string),
                    reason,
                    fork_block: test.get("fork_block_number").and_then(Value::as_u64),
                },
            );
            let holder = test
                .get("decoded_logs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find_map(|line| HOLDER_LOG.captures(line)?.get(1)?.as_str().parse::<Address>().ok());
            if let Some(holder) = holder {
                replayed.holders.insert(name, holder);
            }
        }
    }
    replayed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn forge_output_becomes_replays_and_holders() {
        let output = json!({"test/Proof.t.sol:ProofTest": {"test_results": {
            "test_replay_O1_updater()": {"status": "Success", "reason": null, "fork_block_number": 122_473_005,
                                          "decoded_logs": ["holder: 0x9F403140Bc0574D7d36eA472b82DAa1Bbd4eF327"]},
            "test_replay_V2_withdrawFee()": {"status": "Failure", "reason": "revert: not owner", "decoded_logs": []}
        }}});
        let replayed = parse(&output);
        assert_eq!(replayed.runs["test_replay_O1_updater"].status.as_deref(), Some("Success"));
        assert_eq!(replayed.runs["test_replay_O1_updater"].fork_block, Some(122_473_005));
        assert_eq!(replayed.runs["test_replay_V2_withdrawFee"].reason.as_deref(), Some("revert: not owner"));
        assert_eq!(
            replayed.holders["test_replay_O1_updater"],
            "0x9F403140Bc0574D7d36eA472b82DAa1Bbd4eF327".parse::<Address>().unwrap()
        );
        assert!(!replayed.holders.contains_key("test_replay_V2_withdrawFee"));
    }

    #[test]
    fn replays_are_found_by_name() {
        assert!(has_replays("function test_replay_O1_x() public {}"));
        assert!(!has_replays("function check_O1_x() public {}"));
    }
}
