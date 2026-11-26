//! Reviewing proven claims, during `ask` and on finished projects.

use kea_claims::review::{Finding, Review, Reviewer};
use kea_core::{Claim, ContractRun, Reviews, Verdict};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The review of a project's kept attempt, next to its `result.json`.
pub const REVIEW_FILE: &str = "review.json";

/// The review of one attempt's proven claims.
#[derive(Clone, Debug, Default)]
pub struct AttemptReview {
    /// Review-file entries keyed `address:claim_id`, one per proven claim. An entry carries a
    /// `verdict` only when the claim loses its `proven`.
    pub entries: BTreeMap<String, Value>,
    /// The reasons proofs were taken away, for the next draft.
    pub notes: Vec<String>,
    /// What the review cost.
    pub cost_usd: f64,
}

impl AttemptReview {
    /// The verdicts the scorer applies.
    pub fn reviews(&self) -> Reviews {
        // Every entry names a verdict this crate wrote, so none can be unknown.
        Reviews::from_json(&self.entries).unwrap_or_default()
    }
}

/// Reviews the proven claims of `run`, whose checks are in `test_source`. Fails closed: a claim
/// the review does not clear, because it was judged wider than its checks, was left out of the
/// answer, or the review could not run, is not counted as proven.
pub async fn review(reviewer: &Reviewer, subject_text: &str, run: &ContractRun, test_source: &str) -> AttemptReview {
    let scored = run.score(test_source, &Reviews::default());
    let proven: Vec<&Claim> = scored
        .claims
        .iter()
        .filter(|judged| judged.verdict == Verdict::Proven && judged.meaningful())
        .map(|judged| &judged.claim)
        .collect();
    let mut out = AttemptReview::default();
    if proven.is_empty() {
        return out;
    }
    let answer = match reviewer.review(subject_text, test_source, &proven).await {
        Ok(billed) => {
            out.cost_usd = billed.cost_usd;
            billed.answer.map_err(|unusable| unusable.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    let (found, missing): (BTreeMap<String, Review>, String) = match answer {
        Ok(answer) => {
            (answer.reviews.into_iter().map(|review| (review.id.clone(), review)).collect(), "not reviewed".into())
        }
        Err(problem) => (BTreeMap::new(), format!("the review could not run: {problem}")),
    };
    for claim in proven {
        let (finding, reason) =
            found.get(&claim.id).map_or((Finding::Doubt, missing.as_str()), |review| (review.verdict, &review.reason));
        let mut entry = json!({"finding": finding.name(), "note": reason});
        if let Some(verdict) = finding.verdict() {
            entry["verdict"] = json!(verdict.name());
            out.notes.push(format!("{} ({}): {reason}", claim.id, finding.name()));
        }
        out.entries.insert(format!("{}:{}", run.address.to_lowercase(), claim.id), entry);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_entries_become_scorer_verdicts_only_where_proven_is_lost() {
        let review = AttemptReview {
            entries: BTreeMap::from([
                ("0xab:T1_mint".to_string(), json!({"finding": "ok", "note": ""})),
                ("0xab:V2_sweep".to_string(), json!({"finding": "mismatch", "note": "", "verdict": "mismatch"})),
            ]),
            ..AttemptReview::default()
        };
        let reviews = review.reviews();
        assert_eq!(reviews.get("0xAB", "V2_sweep"), Some(Verdict::Mismatch));
        assert_eq!(reviews.get("0xab", "T1_mint"), None);
    }
}
