//! One contract's run: what the drafter claimed, what the prover and the replay said, and the
//! verdicts and score that follow.

use crate::claim::Claim;
use crate::solidity::{TestFile, covers, uses_cheats};
use crate::verdict::{Coverage, Evidence, Holder, Outcome, Replay, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// The pass mark: proven claims a contract needs.
pub const MIN_PROVEN: usize = 3;
/// The pass mark: prover CPU seconds a contract may use.
pub const MAX_PROVE_SECONDS: u64 = 600;

/// What halmos reported for one `check_` function.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HalmosCheck {
    /// Halmos's own exit code for the check: 0 passed, 1 found a counterexample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exitcode: Option<i64>,
    /// Paths explored: total, successful, blocked.
    #[serde(default)]
    pub paths: Option<Vec<u64>>,
    /// Counterexample models found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Value>,
    /// Time spent, as halmos reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<Value>,
    /// `"pass"`, `"counterexample"`, or another word for anything else.
    #[serde(default)]
    pub verdict: Option<String>,
}

impl HalmosCheck {
    /// A result from halmos's exit code for the check.
    pub fn from_exit(
        exitcode: Option<i64>,
        paths: Option<Vec<u64>>,
        models: Option<Value>,
        time: Option<Value>,
    ) -> Self {
        let verdict = match exitcode {
            Some(0) => "pass",
            Some(1) => "counterexample",
            _ => "unknown",
        };
        Self { exitcode, paths, models, time, verdict: Some(verdict.to_string()) }
    }
}

impl HalmosCheck {
    fn outcome(&self) -> Outcome {
        match self.verdict.as_deref() {
            Some("pass") => Outcome::Pass,
            Some("counterexample") => Outcome::Counterexample,
            _ => Outcome::Other,
        }
    }

    fn successful_paths(&self) -> u64 {
        self.paths.as_deref().and_then(|paths| paths.get(1).copied()).unwrap_or(0)
    }
}

/// What the fork replay reported for one `test_replay_` function.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReplayRun {
    /// `"Success"` when the replayed transaction did what the counterexample said.
    #[serde(default)]
    pub status: Option<String>,
    /// Why it failed, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The block the fork was taken at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_block: Option<u64>,
}

/// Who holds a replayed power, as written by the replay step.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HolderRecord {
    /// The account the replay logged as the power's holder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// `"EOA"`, `"Safe …"`, `"none"`, `"unknown"`, or a contract description.
    #[serde(default)]
    pub kind: Option<String>,
}

/// One drafting attempt.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attempt {
    /// Build and prove status of this attempt.
    #[serde(default)]
    pub status: Option<BTreeMap<String, Value>>,
    /// Everything else recorded about the attempt: its number, model, cost, tokens, proofs.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A contract's run as stored in `result.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContractRun {
    /// The contract address.
    pub address: String,
    /// Source unit and contract name, `path:Name`.
    pub name: String,
    /// The family the contract was picked as (vault, token, pool, ...).
    #[serde(default)]
    pub kind: Option<String>,
    /// The claims of the kept attempt.
    pub claims: Vec<Claim>,
    /// Halmos results by check name.
    #[serde(default)]
    pub halmos: BTreeMap<String, HalmosCheck>,
    /// Replay results by replay name.
    #[serde(default)]
    pub replay: BTreeMap<String, ReplayRun>,
    /// Holders by replay name. Absent in runs that never resolved holders.
    #[serde(default)]
    pub holders: Option<BTreeMap<String, Option<HolderRecord>>>,
    /// Build and prove status of the kept attempt.
    #[serde(default)]
    pub status: Option<BTreeMap<String, Value>>,
    /// Every drafting attempt.
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    /// What drafting would have cost at API prices.
    #[serde(default)]
    pub cost_usd: f64,
}

/// Manual review entries keyed `address:claim_id` (address in lower case). Entries without a
/// verdict, and notes such as `_about`, are ignored.
#[derive(Clone, Debug, Default)]
pub struct Reviews {
    verdicts: BTreeMap<String, Verdict>,
}

/// A review file named a verdict the scorer does not know.
#[derive(Debug, thiserror::Error)]
#[error("review entry {key} names an unknown verdict {name:?}")]
pub struct UnknownReviewVerdict {
    /// The entry's key.
    pub key: String,
    /// The verdict it named.
    pub name: String,
}

impl Reviews {
    /// Reads a review file's JSON object.
    ///
    /// # Errors
    /// When an entry names a verdict that does not exist.
    pub fn from_json(file: &BTreeMap<String, Value>) -> Result<Self, UnknownReviewVerdict> {
        let mut verdicts = BTreeMap::new();
        for (key, entry) in file {
            let Some(name) = entry.get("verdict").and_then(Value::as_str) else { continue };
            let verdict = Verdict::from_name(name)
                .ok_or_else(|| UnknownReviewVerdict { key: key.clone(), name: name.to_string() })?;
            verdicts.insert(key.clone(), verdict);
        }
        Ok(Self { verdicts })
    }

    /// The reviewed verdict of one claim, if any.
    pub fn get(&self, address: &str, claim_id: &str) -> Option<Verdict> {
        self.verdicts.get(&format!("{}:{claim_id}", address.to_lowercase())).copied()
    }

    /// Records `verdict` for one claim, replacing an earlier entry.
    pub fn insert(&mut self, address: &str, claim_id: &str, verdict: Verdict) {
        self.verdicts.insert(format!("{}:{claim_id}", address.to_lowercase()), verdict);
    }

    /// These reviews with `over` laid on top: where both decide a claim, `over` wins.
    #[must_use]
    pub fn under(mut self, over: &Reviews) -> Self {
        self.verdicts.extend(over.verdicts.iter().map(|(key, verdict)| (key.clone(), *verdict)));
        self
    }
}

/// A claim with its verdict.
#[derive(Clone, Debug)]
pub struct Judged {
    /// The claim.
    pub claim: Claim,
    /// Its verdict.
    pub verdict: Verdict,
}

impl Judged {
    /// Whether the claim counts towards the score.
    pub fn meaningful(&self) -> bool {
        self.claim.meaningful()
    }
}

/// A contract's verdicts and score.
#[derive(Clone, Debug)]
pub struct Scored {
    /// The contract name without its source path.
    pub name: String,
    /// The family it was picked as.
    pub kind: Option<String>,
    /// Drafting attempts made.
    pub drafts: usize,
    /// Whether the kept attempt compiled.
    pub built: bool,
    /// Prover CPU seconds (wall seconds for runs that did not measure CPU).
    pub prove_seconds: u64,
    /// Drafting cost at API prices.
    pub cost_usd: f64,
    /// Every claim with its verdict.
    pub claims: Vec<Judged>,
}

impl Scored {
    fn count(&self, keep: impl Fn(&Judged) -> bool) -> usize {
        self.claims.iter().filter(|judged| keep(judged)).count()
    }

    /// Meaningful claims proven.
    pub fn proven(&self) -> usize {
        self.count(|j| j.verdict == Verdict::Proven && j.meaningful())
    }

    /// Meaningful replayed powers held by an account a person controls.
    pub fn findings(&self) -> usize {
        self.count(|j| j.verdict == Verdict::Finding && j.meaningful())
    }

    /// Counterexamples dropped as not evidence.
    pub fn dropped(&self) -> usize {
        self.count(|j| j.verdict.is_dropped())
    }

    /// Counterexamples held back because they did not replay.
    pub fn unreplayed(&self) -> usize {
        self.count(|j| j.verdict == Verdict::FindingUnreplayed)
    }

    /// Passes that are not proofs.
    pub fn weak(&self) -> usize {
        self.count(|j| j.verdict.is_weak())
    }

    /// Claims the prover gave no usable answer for.
    pub fn unknown(&self) -> usize {
        self.count(|j| j.verdict == Verdict::Unknown)
    }

    /// Whether the contract meets the pass mark.
    pub fn passes(&self) -> bool {
        self.proven() >= MIN_PROVEN && self.prove_seconds <= MAX_PROVE_SECONDS
    }
}

impl ContractRun {
    /// Judges every claim against the run's evidence and the generated test file.
    pub fn judge(&self, test_source: &str, reviews: &Reviews) -> Vec<Judged> {
        let file = TestFile::parse(test_source);
        let symbolic = file.symbolic_checks();
        let scopes_declared = self.claims.iter().any(|claim| claim.scope.is_some());
        let missing = HalmosCheck::default();
        self.claims
            .iter()
            .map(|claim| {
                let halmos = self.halmos.get(&claim.check).unwrap_or(&missing);
                let replay_name = claim.replay.as_deref().unwrap_or("");
                let check_text = file.running_text(&claim.check);
                let evidence = Evidence {
                    outcome: halmos.outcome(),
                    successful_paths: halmos.successful_paths(),
                    symbolic: symbolic.contains(&claim.check),
                    check_text: &check_text,
                    coverage: Coverage::of(scopes_declared, covers(&claim.scope(), &check_text)),
                    replay: Replay::of(
                        self.replay.get(replay_name).and_then(|r| r.status.as_deref()) == Some("Success"),
                        uses_cheats(&file.running_text(replay_name)),
                    ),
                    holder: self.holder(replay_name),
                };
                let verdict = reviews.get(&self.address, &claim.id).unwrap_or_else(|| evidence.verdict());
                Judged { claim: claim.clone(), verdict }
            })
            .collect()
    }

    fn holder(&self, replay_name: &str) -> Holder {
        match &self.holders {
            None => Holder::NotResolved,
            Some(holders) => {
                let kind = holders.get(replay_name).and_then(Option::as_ref).and_then(|h| h.kind.as_deref());
                Holder::from_kind(kind.unwrap_or(""))
            }
        }
    }

    /// The status of the kept attempt: the run's own, else the last attempt's.
    fn status(&self) -> BTreeMap<String, Value> {
        self.status
            .clone()
            .filter(|status| !status.is_empty())
            .or_else(|| self.attempts.last().and_then(|attempt| attempt.status.clone()))
            .unwrap_or_default()
    }

    /// Scores the run against the pass mark.
    pub fn score(&self, test_source: &str, reviews: &Reviews) -> Scored {
        let status = self.status();
        Scored {
            name: self.name.rsplit(':').next().unwrap_or(&self.name).to_string(),
            kind: self.kind.clone(),
            drafts: self.attempts.len(),
            built: status.get("build").and_then(Value::as_str) == Some("0"),
            prove_seconds: first_count(&status, &["prove_cpu", "prove_s"]),
            cost_usd: self.cost_usd,
            claims: self.judge(test_source, reviews),
        }
    }
}

/// The first of `keys` with a set value, read as whole seconds; zero when none has one. A set
/// value is a non-empty string (even `"0"`) or a non-zero number, as the published scorer reads it.
fn first_count(status: &BTreeMap<String, Value>, keys: &[&str]) -> u64 {
    keys.iter()
        .filter_map(|key| status.get(*key))
        .find_map(|value| match value {
            Value::String(text) if !text.is_empty() => Some(text.trim().parse().unwrap_or(0)),
            Value::Number(number) => number.as_u64().filter(|seconds| *seconds != 0),
            _ => None,
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "function setUp() public { x = 1; }\n\
        function check_A() public { uint a = svm.createUint256('a'); assert(a >= 0); }\n\
        function check_B() public { assert(true); }\n\
        function pick() internal returns (uint) { return svm.createUint256('p'); }\n\
        function check_C() public { uint p = pick(); assert(p >= 0); }\n\
        function test_replay_C() public { _fake(); }\n\
        function _fake() internal { vm.store(LIVE, 0, 0); }\n\
        function check_D() public { address c = svm.createAddress('c'); bytes memory d = svm.createCalldata('X'); assert(t.fee() <= 10); }\n\
        function check_E() public { address c = svm.createAddress('c'); vm.prank(c); t.claimBond(); assert(true); }\n\
        function check_F() public { address c = svm.createAddress('c'); bytes32 h = address(t).codehash; assert(address(t).codehash == h); }\n\
        function test_replay_G() public { emit log_named_address(\"holder\", o); }";

    fn verdicts(run: &str) -> Vec<Verdict> {
        let run: ContractRun = serde_json::from_str(run).unwrap();
        run.judge(FILE, &Reviews::default()).into_iter().map(|j| j.verdict).collect()
    }

    #[test]
    fn the_first_run_format_scores_as_before() {
        let got = verdicts(
            r#"{"address":"0xAb","name":"a.sol:A","claims":[
                {"id":"V1_x","plain":"","check":"check_B"},{"id":"V2_y","plain":"","check":"check_A"},
                {"id":"O1_z","plain":"","check":"check_C","replay":"test_replay_C"}],
              "halmos":{"check_B":{"verdict":"pass","paths":[1,1,0]},"check_A":{"verdict":"pass","paths":[3,0,3]},
                        "check_C":{"verdict":"counterexample"}},
              "replay":{"test_replay_C":{"status":"Success"}}}"#,
        );
        assert_eq!(got, [Verdict::Concrete, Verdict::Vacuous, Verdict::FindingCheat]);
    }

    #[test]
    fn scoped_claims_and_resolved_holders_score_as_before() {
        let run = r#"{"address":"0xCd","name":"b.sol:B","claims":[
                {"id":"H1_fee","plain":"","check":"check_D","scope":"any"},
                {"id":"V2_out","plain":"","check":"check_E","scope":"any"},
                {"id":"O1_x","plain":"","check":"check_E","scope":"claimBond"},
                {"id":"P1_up","plain":"","check":"check_F","scope":"any"},
                {"id":"O1_g","plain":"","check":"check_G","scope":"any","replay":"test_replay_G"}],
              "halmos":{"check_D":{"verdict":"pass","paths":[2,2,0]},"check_E":{"verdict":"pass","paths":[2,2,0]},
                        "check_F":{"verdict":"pass","paths":[2,2,0]},"check_G":{"verdict":"counterexample"}},
              "replay":{"test_replay_G":{"status":"Success"}},"holders":{"test_replay_G":{"kind":"contract"}}}"#;
        assert_eq!(
            verdicts(run),
            [Verdict::Proven, Verdict::Narrow, Verdict::Proven, Verdict::Trivial, Verdict::ContractHeld]
        );
        let safe = run.replace(r#"{"kind":"contract"}"#, r#"{"kind":"Safe 2"}"#);
        assert_eq!(verdicts(&safe)[4], Verdict::Finding);
        let empty = run.replace(r#"{"test_replay_G":{"kind":"contract"}}"#, "{}");
        assert_eq!(verdicts(&empty)[4], Verdict::FindingNoholder);
    }

    #[test]
    fn reviews_have_the_last_word() {
        let run: ContractRun = serde_json::from_str(
            r#"{"address":"0xAB","name":"c.sol:C","claims":[{"id":"V1_x","plain":"","check":"check_A"}],
                "halmos":{"check_A":{"verdict":"pass","paths":[1,1,0]}}}"#,
        )
        .unwrap();
        let file: BTreeMap<String, Value> = serde_json::from_str(
            r#"{"_about":"notes","0xab:V1_x":{"verdict":"mismatch","why":"wrong function"},"0xab:V2_y":{"holder":"x"}}"#,
        )
        .unwrap();
        let reviews = Reviews::from_json(&file).unwrap();
        assert_eq!(run.judge(FILE, &reviews)[0].verdict, Verdict::Mismatch);
        let bad: BTreeMap<String, Value> = serde_json::from_str(r#"{"0xab:V1_x":{"verdict":"great"}}"#).unwrap();
        assert!(Reviews::from_json(&bad).is_err());
    }

    #[test]
    fn scores_read_cpu_seconds_and_fall_back_to_wall_time() {
        let run: ContractRun = serde_json::from_str(
            r#"{"address":"0x1","name":"src/V.sol:Vault","claims":[],"attempts":[
                {"status":{"build":"0","prove_s":"700"}}],"status":{}}"#,
        )
        .unwrap();
        let scored = run.score("", &Reviews::default());
        assert_eq!(scored.name, "Vault");
        assert!(scored.built);
        assert_eq!(scored.prove_seconds, 700);
        assert!(!scored.passes());
        let mut status = BTreeMap::new();
        status.insert("prove_cpu".to_string(), Value::String("18".into()));
        status.insert("prove_s".to_string(), Value::String("30".into()));
        assert_eq!(first_count(&status, &["prove_cpu", "prove_s"]), 18);
    }
}
