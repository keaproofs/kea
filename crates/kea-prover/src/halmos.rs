//! Symbolic runs with halmos.

use crate::process::{self, Finished, Limits, Run};
use kea_core::run::HalmosCheck;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// How halmos runs.
#[derive(Clone, Debug)]
pub struct Options {
    /// The test contract holding the checks.
    pub contract: String,
    /// The prefix of the functions to prove.
    pub function_prefix: String,
    /// How many times loops are unrolled. Proofs are bounded by it and must say so.
    pub loop_bound: u32,
    /// Solver time per assertion, in halmos's own notation.
    pub solver_timeout_assertion: String,
    /// Solver threads; one per run keeps several runs on one machine fair.
    pub solver_threads: u32,
    /// The cap for the whole contract.
    pub limits: Limits,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            contract: "ProofTest".into(),
            function_prefix: "check_".into(),
            loop_bound: 3,
            solver_timeout_assertion: "60s".into(),
            solver_threads: 1,
            limits: Limits { wall: Duration::from_secs(900), grace: Duration::from_secs(30) },
        }
    }
}

/// The checks halmos decided, and how the run went.
#[derive(Clone, Debug)]
pub struct Proved {
    /// Results by check name.
    pub checks: BTreeMap<String, HalmosCheck>,
    /// The run itself.
    pub run: Finished,
}

/// Proves every check of the project.
///
/// # Errors
/// When halmos cannot be started.
pub async fn prove(project: &Path, options: &Options) -> std::io::Result<Proved> {
    let output = project.join("halmos.json");
    let _ = std::fs::remove_file(&output);
    let run = process::run(Run {
        program: "halmos",
        args: vec![
            "--root".into(),
            ".".into(),
            "--contract".into(),
            options.contract.clone(),
            "--function".into(),
            options.function_prefix.clone(),
            "--loop".into(),
            options.loop_bound.to_string(),
            "--solver-timeout-assertion".into(),
            options.solver_timeout_assertion.clone(),
            "--solver-threads".into(),
            options.solver_threads.to_string(),
            "--json-output".into(),
            "halmos.json".into(),
        ],
        dir: project,
        env: vec![],
        stdout: project.join("halmos.log"),
        stderr: project.join("halmos.err.log"),
        limits: options.limits,
    })
    .await?;
    let checks = std::fs::read_to_string(&output)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .map(|json| parse(&json))
        .unwrap_or_default();
    Ok(Proved { checks, run })
}

/// Results by check name from halmos's JSON output (`test_results` → contract → tests).
pub fn parse(json: &Value) -> BTreeMap<String, HalmosCheck> {
    let mut checks = BTreeMap::new();
    let contracts = json.get("test_results").and_then(Value::as_object).into_iter().flat_map(|c| c.values());
    for test in contracts.filter_map(Value::as_array).flatten() {
        let name = test.get("name").and_then(Value::as_str).unwrap_or("");
        let name = name.split('(').next().unwrap_or(name).to_string();
        let paths = test
            .get("num_paths")
            .and_then(Value::as_array)
            .map(|paths| paths.iter().filter_map(Value::as_u64).collect());
        let check = HalmosCheck::from_exit(
            test.get("exitcode").and_then(Value::as_i64),
            paths,
            test.get("num_models").cloned().filter(|v| !v.is_null()),
            test.get("time").cloned().filter(|v| !v.is_null()),
        );
        checks.insert(name, check);
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn halmos_output_becomes_check_results() {
        let output = json!({"exitcode": 1, "test_results": {"test/Proof.t.sol:ProofTest": [
            {"name": "check_F1_accumulate()", "exitcode": 0, "num_models": 0, "num_paths": [1, 1, 0], "time": [0.02, 0.02, 0.0]},
            {"name": "check_O1_updater(address)", "exitcode": 1, "num_models": 1, "num_paths": [2, 1, 0]},
            {"name": "check_V1_timeout()", "exitcode": 2}
        ]}});
        let checks = parse(&output);
        assert_eq!(checks["check_F1_accumulate"].verdict.as_deref(), Some("pass"));
        assert_eq!(checks["check_F1_accumulate"].paths.as_deref(), Some(&[1, 1, 0][..]));
        assert_eq!(checks["check_O1_updater"].verdict.as_deref(), Some("counterexample"));
        assert_eq!(checks["check_V1_timeout"].verdict.as_deref(), Some("unknown"));
        assert!(parse(&json!({})).is_empty());
    }
}
