//! Reviewing proven claims: do the plain words say only what the checks proved?
//!
//! The prover settles the assertions; it cannot read the sentence printed next to them. A hand
//! check of the first model eval found 8 of 39 proven claims worded wider than their checks:
//! "nobody can" over a check that tried two callers, or "nothing gets stuck" over a router mock
//! that never pays out. A second model reads each proven claim against its checks and the
//! contract, and every claim that would mislead a careful reader loses its `proven`.

use crate::Billed;
use crate::messages::{ApiError, Client};
use crate::model::Model;
use kea_core::{Claim, Verdict};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The reviewer's instructions.
pub const SYSTEM: &str = include_str!("prompts/reviewer_system.md");
/// Judging words against code is reasoning-heavy, as drafting is.
const EFFORT: &str = "high";
/// Room for the thinking and one short reason per claim.
const MAX_TOKENS: u32 = 32_000;

/// What the reviewer decided about one claim.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Finding {
    /// The checks establish the words.
    Ok,
    /// One sample instance, which the contract treats like every other.
    NarrowOk,
    /// The words claim more than the checks prove.
    Mismatch,
    /// The reviewer could not tell.
    Doubt,
}

impl Finding {
    /// The verdict that replaces `proven`, or none when the proof stands. A doubt is not shown as
    /// proven either: `narrow` reads "the words may be wider than the check".
    pub fn verdict(self) -> Option<Verdict> {
        match self {
            Self::Ok | Self::NarrowOk => None,
            Self::Mismatch => Some(Verdict::Mismatch),
            Self::Doubt => Some(Verdict::Narrow),
        }
    }

    /// The name used in the output schema and review files.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NarrowOk => "narrow-ok",
            Self::Mismatch => "mismatch",
            Self::Doubt => "doubt",
        }
    }
}

/// One claim's review.
#[derive(Clone, Debug, Deserialize)]
pub struct Review {
    /// The claim reviewed.
    pub id: String,
    /// The decision.
    pub verdict: Finding,
    /// Why, naming the check's limit.
    pub reason: String,
}

/// The reviewer's answer.
#[derive(Clone, Debug, Deserialize)]
pub struct Reviewed {
    /// One review per claim.
    pub reviews: Vec<Review>,
}

/// Structured-output schema for an answer.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["reviews"],
        "properties": {"reviews": {"type": "array", "items": {
            "type": "object",
            "additionalProperties": false,
            "required": ["id", "verdict", "reason"],
            "properties": {
                "id": {"type": "string"},
                "verdict": {"type": "string", "enum": ["ok", "narrow-ok", "mismatch", "doubt"]},
                "reason": {"type": "string"},
            },
        }}},
    })
}

/// Reviews proven claims with one model.
#[derive(Clone, Debug)]
pub struct Reviewer {
    client: Client,
    model: Model,
}

impl Reviewer {
    /// A reviewer on `model`.
    pub fn new(client: Client, model: Model) -> Self {
        Self { client, model }
    }

    /// Reviews `claims`, all proven by `test_source`, against the subject in `subject_text`.
    ///
    /// # Errors
    /// When no answer was billed: see [`ApiError`].
    pub async fn review(
        &self,
        subject_text: &str,
        test_source: &str,
        claims: &[&Claim],
    ) -> Result<Billed<Reviewed>, ApiError> {
        let reply = self.client.send(self.model, self.request(subject_text, test_source, claims)).await?;
        Ok(Billed::from_reply(reply, self.model))
    }

    fn request(&self, subject_text: &str, test_source: &str, claims: &[&Claim]) -> Value {
        let mut listed = String::from("PROVEN CLAIMS\n");
        for claim in claims {
            let scope = claim.scope.as_deref().filter(|scope| !scope.is_empty()).unwrap_or("any");
            let _ = writeln!(listed, "- {}: {} [scope: {scope}] (checks: {})", claim.id, claim.plain, claim.check);
        }
        json!({
            "model": self.model.id(),
            "max_tokens": MAX_TOKENS,
            "system": [{"type": "text", "text": SYSTEM, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": subject_text, "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": format!("TEST FILE\n{test_source}")},
                {"type": "text", "text": listed},
            ]}],
            "output_config": {"effort": EFFORT, "format": {"type": "json_schema", "schema": schema()}},
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str, scope: Option<&str>) -> Claim {
        serde_json::from_value(
            json!({"id": id, "plain": "Nobody can mint", "check": format!("check_{id}"), "scope": scope}),
        )
        .unwrap()
    }

    #[test]
    fn requests_list_every_claim_with_its_checks_and_scope() {
        let reviewer = Reviewer::new(Client::new("test".into()), Model::Sonnet);
        let (a, b) = (claim("T1_mint", Some("any")), claim("P2_init", Some("initialize")));
        let request = reviewer.request("SUBJECT", "contract ProofTest {}", &[&a, &b]);
        assert_eq!(request["model"], "claude-sonnet-5-5");
        let content = request["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(content[1]["text"], "TEST FILE\ncontract ProofTest {}");
        let listed = content[2]["text"].as_str().unwrap();
        assert!(listed.contains("- T1_mint: Nobody can mint [scope: any] (checks: check_T1_mint)"));
        assert!(listed.contains("[scope: initialize]"));
        assert_eq!(request["output_config"]["format"]["schema"], schema());
    }

    #[test]
    fn only_mismatches_and_doubts_take_away_proven() {
        let answer: Reviewed = serde_json::from_value(json!({"reviews": [
            {"id": "a", "verdict": "ok", "reason": ""},
            {"id": "b", "verdict": "narrow-ok", "reason": ""},
            {"id": "c", "verdict": "mismatch", "reason": ""},
            {"id": "d", "verdict": "doubt", "reason": ""},
        ]}))
        .unwrap();
        let verdicts: Vec<Option<Verdict>> = answer.reviews.iter().map(|review| review.verdict.verdict()).collect();
        assert_eq!(verdicts, [None, None, Some(Verdict::Mismatch), Some(Verdict::Narrow)]);
        assert!(
            serde_json::from_value::<Reviewed>(json!({"reviews": [{"id": "e", "verdict": "fine", "reason": ""}]}))
                .is_err()
        );
    }
}
