//! A small JSON-RPC client for Robinhood Chain, and the account facts Kea reads with it.
//!
//! Kea needs only a handful of calls: storage slots (to resolve proxies), code and balances (to
//! tell people from contracts), and plain `eth_call`s (to read a Safe's threshold). A full
//! Ethereum client library would be most of the build time for a few hundred lines of use.

use alloy_primitives::{Address, B256, Bytes, U256};
use serde_json::{Value, json};
use std::str::FromStr;
use std::time::Duration;

/// Robinhood Chain's chain id.
pub const ROBINHOOD_CHAIN_ID: u64 = 4663;
/// The public Robinhood Chain RPC endpoint.
pub const ROBINHOOD_RPC: &str = "https://rpc.mainnet.chain.robinhood.com";

/// The public endpoint sits behind Cloudflare, which refuses some non-browser user agents.
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131 Safari/537.36";

/// Why an RPC call failed.
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    /// The request never got a usable HTTP answer, even after retries.
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    /// The node answered with a JSON-RPC error.
    #[error("node error {code}: {message}")]
    Node {
        /// The JSON-RPC error code.
        code: i64,
        /// The node's message.
        message: String,
    },
    /// The node answered, but not in the shape the method promises.
    #[error("unexpected answer to {method}: {detail}")]
    Shape {
        /// The method called.
        method: &'static str,
        /// What was wrong.
        detail: String,
    },
}

/// A JSON-RPC client. Transport failures and rate limits are retried with backoff; errors the
/// node itself returns are not.
#[derive(Clone, Debug)]
pub struct Rpc {
    url: String,
    http: reqwest::Client,
    attempts: u32,
}

impl Rpc {
    /// A client for `url`.
    ///
    /// # Panics
    /// When the TLS backend cannot be initialised, which only happens on a broken system.
    pub fn new(url: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client with built-in TLS");
        Self { url: url.into(), http, attempts: 5 }
    }

    /// A client for the public Robinhood Chain endpoint.
    pub fn robinhood() -> Self {
        Self::new(ROBINHOOD_RPC)
    }

    async fn request(&self, method: &'static str, params: Value) -> Result<Value, RpcError> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
        let mut delay = Duration::from_secs(1);
        let mut attempt = 1;
        let answer: Value = loop {
            let sent = self.http.post(&self.url).json(&body).send().await;
            let retry = match sent {
                Ok(response) if response.status().is_success() => match response.json().await {
                    Ok(answer) => break answer,
                    Err(error) => error,
                },
                Ok(response) => match response.error_for_status() {
                    Err(error) => error,
                    Ok(response) => break response.json().await?,
                },
                Err(error) => error,
            };
            if attempt >= self.attempts {
                return Err(retry.into());
            }
            attempt += 1;
            tokio::time::sleep(delay).await;
            delay *= 2;
        };
        if let Some(error) = answer.get("error") {
            return Err(RpcError::Node {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                message: error.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
            });
        }
        answer.get("result").cloned().ok_or(RpcError::Shape { method, detail: "no result".into() })
    }

    async fn hex<T: FromStr>(&self, method: &'static str, params: Value) -> Result<T, RpcError> {
        let result = self.request(method, params).await?;
        let text =
            result.as_str().ok_or_else(|| RpcError::Shape { method, detail: format!("not a string: {result}") })?;
        text.parse().map_err(|_| RpcError::Shape { method, detail: format!("not hex of the right size: {text}") })
    }

    /// The word stored at `slot` of `address`, at the latest block.
    ///
    /// # Errors
    /// See [`RpcError`].
    pub async fn storage_at(&self, address: Address, slot: B256) -> Result<B256, RpcError> {
        self.hex("eth_getStorageAt", json!([address, slot, "latest"])).await
    }

    /// The ETH balance of `address`, in wei.
    ///
    /// # Errors
    /// See [`RpcError`].
    pub async fn balance(&self, address: Address) -> Result<U256, RpcError> {
        self.hex("eth_getBalance", json!([address, "latest"])).await
    }

    /// The code deployed at `address`; empty for an account a person controls.
    ///
    /// # Errors
    /// See [`RpcError`].
    pub async fn code(&self, address: Address) -> Result<Bytes, RpcError> {
        self.hex("eth_getCode", json!([address, "latest"])).await
    }

    /// How many transactions `address` has sent.
    ///
    /// # Errors
    /// See [`RpcError`].
    pub async fn transaction_count(&self, address: Address) -> Result<U256, RpcError> {
        self.hex("eth_getTransactionCount", json!([address, "latest"])).await
    }

    /// The return data of a read-only call.
    ///
    /// # Errors
    /// See [`RpcError`]; a call that reverts is a [`RpcError::Node`].
    pub async fn call(&self, to: Address, data: &[u8]) -> Result<Bytes, RpcError> {
        self.hex("eth_call", json!([{ "to": to, "data": Bytes::copy_from_slice(data) }, "latest"])).await
    }
}

/// Who can use a power held at an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountKind {
    /// The zero address: the power was renounced.
    Nobody,
    /// No code, and it has sent transactions: an account a person controls.
    Person,
    /// No code and no transaction ever sent: an address nobody has used, such as one a test made up.
    Unused,
    /// A Safe multisig with this many required signatures.
    Safe(u64),
    /// Any other contract.
    Contract,
    /// The chain could not be read.
    Unknown,
}

impl AccountKind {
    /// The kind as the replay step records it: `none`, `EOA`, `unused`, `Safe N`, `contract`, `unknown`.
    pub fn label(&self) -> String {
        match self {
            Self::Nobody => "none".into(),
            Self::Person => "EOA".into(),
            Self::Unused => "unused".into(),
            Self::Safe(threshold) => format!("Safe {threshold}"),
            Self::Contract => "contract".into(),
            Self::Unknown => "unknown".into(),
        }
    }
}

/// `getThreshold()`: a Safe answers with the number of signatures it needs.
const GET_THRESHOLD: [u8; 4] = [0xe7, 0x52, 0x35, 0xb8];

/// Reads who stands behind `address`.
pub async fn account_kind(rpc: &Rpc, address: Address) -> AccountKind {
    if address.is_zero() {
        return AccountKind::Nobody;
    }
    let Ok(code) = rpc.code(address).await else { return AccountKind::Unknown };
    if code.is_empty() {
        // A real admin has acted on chain. An address that never has is one a test invented, so a
        // power "held" there is no admin's power: it may be a hole anyone can use.
        return match rpc.transaction_count(address).await {
            Ok(count) if count.is_zero() => AccountKind::Unused,
            Ok(_) => AccountKind::Person,
            Err(_) => AccountKind::Unknown,
        };
    }
    match rpc.call(address, &GET_THRESHOLD).await {
        Ok(word) if word.len() == 32 => {
            let threshold = U256::from_be_slice(&word);
            match u64::try_from(threshold) {
                Ok(threshold) if threshold > 0 => AccountKind::Safe(threshold),
                _ => AccountKind::Contract,
            }
        }
        _ => AccountKind::Contract,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_use_the_recorded_labels() {
        assert_eq!(AccountKind::Nobody.label(), "none");
        assert_eq!(AccountKind::Person.label(), "EOA");
        assert_eq!(AccountKind::Unused.label(), "unused");
        assert_eq!(AccountKind::Safe(2).label(), "Safe 2");
        assert_eq!(AccountKind::Contract.label(), "contract");
        assert_eq!(AccountKind::Unknown.label(), "unknown");
    }

    #[test]
    fn the_threshold_selector_is_get_threshold() {
        let selector = &alloy_primitives::keccak256("getThreshold()")[..4];
        assert_eq!(selector, GET_THRESHOLD);
    }
}
