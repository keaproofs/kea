//! A streaming client for the Claude Messages API.
//!
//! Rust has no official Anthropic SDK, so this speaks the documented HTTP protocol directly.
//! Drafts are long (a test file plus the model's thinking), so every request streams: a
//! non-streaming request that large risks an HTTP timeout.

use crate::model::{Model, Usage};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
/// Lets the API re-run a declined request on another model inside the same call.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Why a request produced no usable answer.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request never got a usable HTTP answer.
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    /// The API answered with an error.
    #[error("API error {status} ({kind}): {message}")]
    Api {
        /// HTTP status.
        status: u16,
        /// The API's error type.
        kind: String,
        /// The API's message.
        message: String,
    },
    /// The event stream ended without a finished message.
    #[error("the stream ended early: {0}")]
    Stream(String),
}

impl ApiError {
    fn retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Api { status, kind, .. } => {
                matches!(status, 429 | 500 | 502 | 503 | 504 | 529) || kind == "overloaded_error"
            }
            Self::Stream(_) => false,
        }
    }
}

/// How a billed answer ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The model finished.
    Done,
    /// The model, and any fallback, declined; the category when the API gave one.
    Declined(String),
    /// The answer reached the output limit and is incomplete.
    CutOff,
}

/// A billed answer: finished, declined or cut off. Each one costs tokens, so none is an error.
#[derive(Clone, Debug)]
pub struct Reply {
    /// The text of the answer (with structured outputs, the JSON document).
    pub text: String,
    /// How it ended.
    pub stop: Stop,
    /// Tokens used, across every model that ran.
    pub usage: Usage,
    /// The model that produced the answer.
    pub served_by: String,
}

/// A Messages API client.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    key: String,
    attempts: u32,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").field("key", &"<hidden>").field("attempts", &self.attempts).finish_non_exhaustive()
    }
}

impl Client {
    /// A client that authenticates with `key`.
    ///
    /// # Panics
    /// When the TLS backend cannot be initialised, which only happens on a broken system.
    pub fn new(key: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(20 * 60))
            .build()
            .expect("HTTP client with built-in TLS");
        Self { http, key, attempts: 4 }
    }

    /// A client from `ANTHROPIC_API_KEY`.
    pub fn from_env() -> Option<Self> {
        std::env::var("ANTHROPIC_API_KEY").ok().filter(|key| !key.is_empty()).map(Self::new)
    }

    /// Sends `body` as a streaming request and collects the answer. Overload and rate limits are
    /// retried with backoff. A declined or cut-off answer is a [`Reply`], because it was billed.
    ///
    /// # Errors
    /// See [`ApiError`].
    pub async fn send(&self, model: Model, mut body: Value) -> Result<Reply, ApiError> {
        body["stream"] = Value::Bool(true);
        if model.has_server_fallback() {
            body["fallbacks"] = Value::String("default".into());
        }
        let mut delay = Duration::from_secs(5);
        let mut attempt = 1;
        loop {
            match self.send_once(model, &body).await {
                Err(error) if error.retryable() && attempt < self.attempts => {
                    tokio::time::sleep(delay).await;
                    delay *= 3;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    async fn send_once(&self, model: Model, body: &Value) -> Result<Reply, ApiError> {
        let mut request =
            self.http.post(ENDPOINT).header("x-api-key", &self.key).header("anthropic-version", API_VERSION).json(body);
        if model.has_server_fallback() {
            request = request.header("anthropic-beta", FALLBACK_BETA);
        }
        let mut response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let answer: Value = response.json().await.unwrap_or(Value::Null);
            return Err(ApiError::Api {
                status: status.as_u16(),
                kind: answer.pointer("/error/type").and_then(Value::as_str).unwrap_or("").to_string(),
                message: answer.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_string(),
            });
        }
        let mut stream = Stream::default();
        let mut pending = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            pending.extend_from_slice(&chunk);
            while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                stream.line(String::from_utf8_lossy(&line).trim_end());
            }
        }
        stream.finish()
    }
}

/// The state of one event stream.
#[derive(Debug, Default)]
struct Stream {
    blocks: BTreeMap<u64, (String, String)>,
    usage: Usage,
    stop_reason: Option<String>,
    refusal: Option<String>,
    model: Option<String>,
    error: Option<(String, String)>,
}

impl Stream {
    fn line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data:") else { return };
        if let Ok(event) = serde_json::from_str::<Value>(data.trim()) {
            self.event(&event);
        }
    }

    fn event(&mut self, event: &Value) {
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if self.model.is_none() {
                    self.model = event.pointer("/message/model").and_then(Value::as_str).map(str::to_string);
                }
                if let Some(usage) = event.pointer("/message/usage") {
                    self.add_input(usage);
                }
            }
            Some("content_block_start") => {
                let block = event.get("content_block").cloned().unwrap_or(Value::Null);
                let kind = block.get("type").and_then(Value::as_str).unwrap_or("").to_string();
                let text = block.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                if kind == "fallback" {
                    // A declined model's partial answer must not prefix the fallback's answer.
                    self.blocks.retain(|_, (kind, _)| kind != "text");
                    self.model =
                        block.pointer("/to/model").and_then(Value::as_str).map(str::to_string).or(self.model.take());
                }
                self.blocks.insert(index, (kind, text));
            }
            Some("content_block_delta") => {
                if let (Some("text_delta"), Some(text)) = (
                    event.pointer("/delta/type").and_then(Value::as_str),
                    event.pointer("/delta/text").and_then(Value::as_str),
                ) && let Some((_, block)) = self.blocks.get_mut(&index)
                {
                    block.push_str(text);
                }
            }
            Some("message_delta") => {
                if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(reason.to_string());
                }
                if let Some(category) = event.pointer("/delta/stop_details/category").and_then(Value::as_str) {
                    self.refusal = Some(category.to_string());
                }
                if let Some(output) = event.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.usage.output = self.usage.output.max(output);
                }
            }
            Some("error") => {
                let kind = event.pointer("/error/type").and_then(Value::as_str).unwrap_or("").to_string();
                let message = event.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_string();
                self.error = Some((kind, message));
            }
            _ => {}
        }
    }

    fn add_input(&mut self, usage: &Value) {
        let read = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        self.usage.input += read("input_tokens");
        self.usage.cache_write += read("cache_creation_input_tokens");
        self.usage.cache_read += read("cache_read_input_tokens");
        self.usage.output = self.usage.output.max(read("output_tokens"));
    }

    fn finish(self) -> Result<Reply, ApiError> {
        if let Some((kind, message)) = self.error {
            return Err(ApiError::Api { status: 200, kind, message });
        }
        let stop = match self.stop_reason.as_deref() {
            Some("refusal") => Stop::Declined(self.refusal.unwrap_or_else(|| "no category".into())),
            Some("max_tokens") => Stop::CutOff,
            Some(_) => Stop::Done,
            None => return Err(ApiError::Stream("no stop reason".into())),
        };
        let text: String =
            self.blocks.values().filter(|(kind, _)| kind == "text").map(|(_, text)| text.as_str()).collect();
        Ok(Reply { text, stop, usage: self.usage, served_by: self.model.unwrap_or_default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> Result<Reply, ApiError> {
        let mut stream = Stream::default();
        for line in lines {
            stream.line(line);
        }
        stream.finish()
    }

    const START: &str = r#"data: {"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":120,"cache_creation_input_tokens":9000,"cache_read_input_tokens":0,"output_tokens":1}}}"#;

    #[test]
    fn text_deltas_become_the_answer_and_usage_is_counted() {
        let reply = feed(&[
            "event: message_start",
            START,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"{\"a\":"}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"1}"}}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4100}}"#,
            r#"data: {"type":"message_stop"}"#,
        ])
        .unwrap();
        assert_eq!(reply.text, r#"{"a":1}"#);
        assert_eq!(reply.stop, Stop::Done);
        assert_eq!(reply.usage, Usage { input: 120, cache_write: 9000, cache_read: 0, output: 4100 });
        assert_eq!(reply.served_by, "claude-opus-5-5");
    }

    #[test]
    fn a_fallback_drops_the_declined_partial_answer() {
        let reply = feed(&[
            START,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":"{\"half"}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"fallback","to":{"model":"claude-opus-4-8"}}}"#,
            r#"data: {"type":"content_block_start","index":2,"content_block":{"type":"text","text":"{}"}}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":10}}"#,
        ])
        .unwrap();
        assert_eq!(reply.text, "{}");
        assert_eq!(reply.served_by, "claude-opus-4-8");
    }

    #[test]
    fn declined_and_cut_off_answers_are_billed_replies_and_stream_errors_are_errors() {
        let declined = feed(&[
            START,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}},"usage":{"output_tokens":300}}"#,
        ])
        .unwrap();
        assert_eq!(declined.stop, Stop::Declined("cyber".into()));
        assert_eq!(declined.usage.output, 300);
        let cut = feed(&[
            START,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":64000}}"#,
        ])
        .unwrap();
        assert_eq!(cut.stop, Stop::CutOff);
        assert_eq!(cut.usage.output, 64_000);
        let overloaded =
            feed(&[START, r#"data: {"type":"error","error":{"type":"overloaded_error","message":"busy"}}"#]);
        assert!(overloaded.as_ref().is_err_and(ApiError::retryable));
        assert!(matches!(feed(&[START]), Err(ApiError::Stream(_))));
    }
}
