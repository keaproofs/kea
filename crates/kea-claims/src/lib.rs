//! Drafting claims and the checks that decide them, with the Claude API.
//!
//! One request carries the instructions and the claim catalog, then the subject (sources, ABI,
//! live facts), then what changes between attempts: a person's question and the feedback from
//! the last run. The first two parts are marked for caching, so retries on the same contract
//! pay full price only for what is new.

pub mod draft;
pub mod messages;
pub mod model;
pub mod prompt;
pub mod review;

use draft::Draft;
use messages::{ApiError, Client, Reply, Stop};
use model::{Model, Usage};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// How hard the drafter thinks. Writing provable checks is reasoning-heavy work.
const EFFORT: &str = "high";
/// Room for the thinking and the test file; streaming keeps this from timing out.
const MAX_TOKENS: u32 = 64_000;

/// Why a billed answer holds no draft.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unusable {
    /// The model, and any fallback, declined.
    #[error("the model declined ({0})")]
    Declined(String),
    /// The answer reached the output limit.
    #[error("the answer was cut off at the output limit")]
    CutOff,
    /// The answer was not a draft.
    #[error("the answer was not a draft: {0}")]
    Malformed(String),
}

/// A billed answer: what it holds, or why it holds nothing, and what it cost either way.
#[derive(Clone, Debug)]
pub struct Billed<T> {
    /// The structured answer, or why there is none.
    pub answer: Result<T, Unusable>,
    /// Tokens used.
    pub usage: Usage,
    /// Cost in US dollars at list prices.
    pub cost_usd: f64,
    /// The model that wrote it (a fallback model when the first one declined).
    pub served_by: String,
}

impl<T: DeserializeOwned> Billed<T> {
    /// Reads a structured-output reply as `T`.
    pub fn from_reply(reply: Reply, model: Model) -> Self {
        let answer = match reply.stop {
            Stop::Done => serde_json::from_str(&reply.text).map_err(|error| Unusable::Malformed(error.to_string())),
            Stop::Declined(category) => Err(Unusable::Declined(category)),
            Stop::CutOff => Err(Unusable::CutOff),
        };
        Self { answer, usage: reply.usage, cost_usd: reply.usage.cost(model), served_by: reply.served_by }
    }
}

/// A billed drafting answer.
pub type Drafted = Billed<Draft>;

/// Writes drafts with one model.
#[derive(Clone, Debug)]
pub struct Drafter {
    client: Client,
    model: Model,
}

impl Drafter {
    /// A drafter on `model`.
    pub fn new(client: Client, model: Model) -> Self {
        Self { client, model }
    }

    /// The model drafting.
    pub fn model(&self) -> Model {
        self.model
    }

    /// Drafts claims and a test file for the subject described by `subject_text`, answering
    /// `question` first when there is one, and fixing what `feedback` reports on a retry.
    ///
    /// # Errors
    /// When no answer was billed: see [`ApiError`].
    pub async fn draft(
        &self,
        subject_text: &str,
        question: Option<&str>,
        feedback: Option<&str>,
    ) -> Result<Drafted, ApiError> {
        let reply = self.client.send(self.model, self.request(subject_text, question, feedback)).await?;
        Ok(Billed::from_reply(reply, self.model))
    }

    fn request(&self, subject_text: &str, question: Option<&str>, feedback: Option<&str>) -> Value {
        let mut content = vec![json!({"type": "text", "text": subject_text, "cache_control": {"type": "ephemeral"}})];
        if let Some(question) = question {
            content.push(json!({"type": "text", "text": format!("QUESTION\n{question}")}));
        }
        if let Some(feedback) = feedback {
            content.push(json!({"type": "text", "text": feedback}));
        }
        json!({
            "model": self.model.id(),
            "max_tokens": MAX_TOKENS,
            "system": [{"type": "text", "text": prompt::SYSTEM, "cache_control": {"type": "ephemeral"}}],
            "messages": [{"role": "user", "content": content}],
            "output_config": {"effort": EFFORT, "format": {"type": "json_schema", "schema": draft::schema()}},
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_put_the_stable_parts_first_and_cache_them() {
        let drafter = Drafter::new(Client::new("test".into()), Model::Opus);
        let request = drafter.request("SUBJECT", Some("can the owner mint?"), Some("PREVIOUS ATTEMPT"));
        assert_eq!(request["model"], "claude-opus-5-5");
        assert_eq!(request["system"][0]["cache_control"]["type"], "ephemeral");
        let content = request["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(content[1]["text"], "QUESTION\ncan the owner mint?");
        assert!(content[2].get("cache_control").is_none());
        assert_eq!(request["output_config"]["format"]["type"], "json_schema");
        assert!(request.get("thinking").is_none(), "adaptive thinking is the default on every drafting model");
    }
}
