//! Verified source code from Sourcify, with Blockscout as the fallback.
//!
//! Everything here comes from whoever verified the contract, so nothing in it is trusted:
//! paths are checked before files are written and settings are validated before they reach a
//! build configuration (see [`crate::project`]).

use alloy_primitives::Address;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const SOURCIFY: &str = "https://sourcify.dev/server/v2/contract";
const BLOCKSCOUT: &str = "https://robinhoodchain.blockscout.com/api/v2/smart-contracts";
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131 Safari/537.36";

/// Where a verified source came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Sourcify's v2 API.
    Sourcify,
    /// The chain's Blockscout explorer.
    Blockscout,
}

/// Compiler settings the contract was verified with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompilerSettings {
    /// Whether the optimizer ran.
    pub optimizer_enabled: bool,
    /// Optimizer runs.
    pub optimizer_runs: u64,
    /// Whether the IR pipeline was used.
    pub via_ir: bool,
    /// The EVM version, when one was set.
    pub evm_version: Option<String>,
    /// Import remappings, as verified.
    pub remappings: Vec<String>,
}

/// A contract's verified source.
#[derive(Clone, Debug)]
pub struct VerifiedSource {
    /// Source files by their source-unit path.
    pub sources: BTreeMap<String, String>,
    /// The full compiler version, such as `0.8.22+commit.4fc1097e`.
    pub compiler_version: String,
    /// How it was compiled.
    pub settings: CompilerSettings,
    /// `path:Name` of the verified contract.
    pub fully_qualified_name: String,
    /// The contract ABI.
    pub abi: Option<Value>,
    /// ABI-encoded constructor arguments.
    pub constructor_args: Option<String>,
    /// Deployment facts, when the source has them.
    pub deployment: Option<Value>,
    /// Where the source came from.
    pub origin: Origin,
}

impl VerifiedSource {
    /// The path of the source unit that defines the verified contract.
    pub fn main_path(&self) -> &str {
        self.fully_qualified_name.rsplit_once(':').map_or(&self.fully_qualified_name, |(path, _)| path)
    }

    /// The verified contract's name.
    pub fn contract_name(&self) -> &str {
        self.fully_qualified_name.rsplit_once(':').map_or(&self.fully_qualified_name, |(_, name)| name)
    }

    /// Names a main source unit that has no file name (`.sol`, as some verifications store
    /// flattened code) after its contract. Unrenamed, it is a hidden file that no import resolves,
    /// so no test of it can build. Known limit: another unit importing it by `.sol` would break; a
    /// flattened source has no such unit.
    fn named(mut self) -> Self {
        let main = self.main_path().to_string();
        let (dir, file) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
        if file == ".sol" {
            let name = self.contract_name().to_string();
            let renamed = if dir.is_empty() { format!("{name}.sol") } else { format!("{dir}/{name}.sol") };
            if !self.sources.contains_key(&renamed)
                && let Some(code) = self.sources.remove(&main)
            {
                self.sources.insert(renamed.clone(), code);
                self.fully_qualified_name = format!("{renamed}:{name}");
            }
        }
        self
    }
}

/// Why no verified source could be read.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// Neither Sourcify nor Blockscout has a verified source for the address.
    #[error("{0} has no verified source")]
    NotVerified(Address),
    /// Blockscout kept failing.
    #[error("explorer request failed: {0}")]
    Http(#[from] reqwest::Error),
    /// An answer was missing something every verified contract has.
    #[error("verified source of {address} is missing {what}")]
    Incomplete {
        /// The contract.
        address: Address,
        /// What was missing.
        what: &'static str,
    },
}

/// Reads verified sources over HTTP.
#[derive(Clone, Debug)]
pub struct Explorer {
    http: reqwest::Client,
    chain_id: u64,
}

impl Explorer {
    /// An explorer client for `chain_id`.
    ///
    /// # Panics
    /// When the TLS backend cannot be initialised, which only happens on a broken system.
    pub fn new(chain_id: u64) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(60))
            .build()
            .expect("HTTP client with built-in TLS");
        Self { http, chain_id }
    }

    /// The verified source of `address`: Sourcify first, then Blockscout.
    ///
    /// # Errors
    /// When neither has it, or Blockscout keeps failing.
    pub async fn fetch(&self, address: Address) -> Result<VerifiedSource, SourceError> {
        if let Some(found) = self.sourcify(address).await {
            return from_sourcify(address, &found).map(VerifiedSource::named);
        }
        let found = self.blockscout(address).await?;
        from_blockscout(address, &found).map(VerifiedSource::named)
    }

    async fn sourcify(&self, address: Address) -> Option<Value> {
        let url = format!(
            "{SOURCIFY}/{}/{address}?fields=sources,compilation,abi,creationBytecode,deployment",
            self.chain_id
        );
        for attempt in 0..2u64 {
            if let Ok(response) = self.http.get(&url).send().await {
                if response.status().is_success() {
                    return response.json().await.ok();
                }
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return None;
                }
            }
            tokio::time::sleep(Duration::from_secs(4 + 4 * attempt)).await;
        }
        None
    }

    async fn blockscout(&self, address: Address) -> Result<Value, SourceError> {
        let url = format!("{BLOCKSCOUT}/{address}");
        let mut last = None;
        for attempt in 0..5u64 {
            let sent = self
                .http
                .get(&url)
                .header("accept", "application/json")
                .header("referer", "https://robinhoodchain.blockscout.com/")
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            match sent {
                Ok(response) => return Ok(response.json().await?),
                Err(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND) => {
                    return Err(SourceError::NotVerified(address));
                }
                Err(error) => last = Some(error),
            }
            tokio::time::sleep(Duration::from_secs(8 + 8 * attempt)).await;
        }
        Err(last.map_or(SourceError::NotVerified(address), SourceError::Http))
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty()).map(str::to_string)
}

fn remappings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn from_sourcify(address: Address, found: &Value) -> Result<VerifiedSource, SourceError> {
    let sources: BTreeMap<String, String> = found
        .get("sources")
        .and_then(Value::as_object)
        .ok_or(SourceError::Incomplete { address, what: "sources" })?
        .iter()
        .filter_map(|(path, file)| Some((path.clone(), file.get("content")?.as_str()?.to_string())))
        .collect();
    let compilation = found.get("compilation").ok_or(SourceError::Incomplete { address, what: "compilation" })?;
    let settings = compilation.get("compilerSettings");
    let optimizer = settings.and_then(|s| s.get("optimizer"));
    Ok(VerifiedSource {
        sources,
        compiler_version: text(compilation.get("compilerVersion"))
            .ok_or(SourceError::Incomplete { address, what: "compiler version" })?,
        settings: CompilerSettings {
            optimizer_enabled: optimizer.and_then(|o| o.get("enabled")).and_then(Value::as_bool).unwrap_or(false),
            optimizer_runs: optimizer.and_then(|o| o.get("runs")).and_then(Value::as_u64).unwrap_or(200),
            via_ir: settings.and_then(|s| s.get("viaIR")).and_then(Value::as_bool).unwrap_or(false),
            evm_version: text(settings.and_then(|s| s.get("evmVersion"))),
            remappings: remappings(settings.and_then(|s| s.get("remappings"))),
        },
        fully_qualified_name: text(compilation.get("fullyQualifiedName"))
            .ok_or(SourceError::Incomplete { address, what: "contract name" })?,
        abi: found.get("abi").cloned(),
        constructor_args: text(found.pointer("/creationBytecode/transformationValues/constructorArguments")),
        deployment: found.get("deployment").cloned().filter(|d| !d.is_null()),
        origin: Origin::Sourcify,
    })
}

fn from_blockscout(address: Address, found: &Value) -> Result<VerifiedSource, SourceError> {
    let main_source = text(found.get("source_code")).ok_or(SourceError::NotVerified(address))?;
    let name = text(found.get("name")).unwrap_or_else(|| "Main".into());
    let main_path = text(found.get("file_path")).unwrap_or_else(|| format!("{name}.sol"));
    let mut sources = BTreeMap::from([(main_path.clone(), main_source)]);
    for extra in found.get("additional_sources").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(path), Some(code)) = (text(extra.get("file_path")), text(extra.get("source_code"))) {
            sources.insert(path, code);
        }
    }
    let settings = found.get("compiler_settings");
    let optimizer = settings.and_then(|s| s.get("optimizer")).filter(|o| o.is_object());
    let (optimizer_enabled, optimizer_runs) = match optimizer {
        Some(o) => (
            o.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            o.get("runs").and_then(Value::as_u64).unwrap_or(200),
        ),
        None => (
            found.get("optimization_enabled").and_then(Value::as_bool).unwrap_or(false),
            found.get("optimization_runs").and_then(Value::as_u64).filter(|runs| *runs != 0).unwrap_or(200),
        ),
    };
    Ok(VerifiedSource {
        sources,
        compiler_version: text(found.get("compiler_version"))
            .map(|v| v.trim_start_matches('v').to_string())
            .ok_or(SourceError::Incomplete { address, what: "compiler version" })?,
        settings: CompilerSettings {
            optimizer_enabled,
            optimizer_runs,
            via_ir: settings.and_then(|s| s.get("viaIR")).and_then(Value::as_bool).unwrap_or(false),
            evm_version: text(settings.and_then(|s| s.get("evmVersion"))).or_else(|| text(found.get("evm_version"))),
            remappings: remappings(settings.and_then(|s| s.get("remappings"))),
        },
        fully_qualified_name: format!("{main_path}:{name}"),
        abi: found.get("abi").cloned().filter(|abi| !abi.is_null()),
        constructor_args: text(found.get("constructor_args")),
        deployment: None,
        origin: Origin::Blockscout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sourcify_answers_become_sources() {
        let found = json!({
            "sources": {"contracts/PriceFeed.sol": {"content": "contract PriceFeed {}"}},
            "compilation": {"compilerVersion": "0.8.22+commit.4fc1097e", "fullyQualifiedName": "contracts/PriceFeed.sol:PriceFeed",
                            "compilerSettings": {"optimizer": {"enabled": true, "runs": 20000}, "evmVersion": "paris",
                                                 "remappings": ["@oz/=lib/oz/"]}},
            "creationBytecode": {"transformationValues": {"constructorArguments": "0x01"}}
        });
        let source = from_sourcify(Address::ZERO, &found).unwrap();
        assert_eq!(source.main_path(), "contracts/PriceFeed.sol");
        assert_eq!(source.contract_name(), "PriceFeed");
        assert!(source.settings.optimizer_enabled);
        assert_eq!(source.settings.optimizer_runs, 20000);
        assert_eq!(source.settings.evm_version.as_deref(), Some("paris"));
        assert_eq!(source.constructor_args.as_deref(), Some("0x01"));
    }

    #[test]
    fn blockscout_answers_become_sources() {
        let found = json!({
            "name": "Vault", "file_path": "src/Vault.sol", "source_code": "contract Vault {}",
            "additional_sources": [{"file_path": "src/Lib.sol", "source_code": "library Lib {}"}],
            "compiler_version": "v0.8.24+commit.e11b9ed9", "optimization_enabled": true, "optimization_runs": 0,
            "evm_version": "default"
        });
        let source = from_blockscout(Address::ZERO, &found).unwrap();
        assert_eq!(source.fully_qualified_name, "src/Vault.sol:Vault");
        assert_eq!(source.sources.len(), 2);
        assert_eq!(source.compiler_version, "0.8.24+commit.e11b9ed9");
        assert_eq!(source.settings.optimizer_runs, 200);
        assert_eq!(source.settings.evm_version.as_deref(), Some("default"));
        assert!(from_blockscout(Address::ZERO, &json!({"name": "X"})).is_err());
    }

    #[test]
    fn a_unit_with_no_file_name_is_named_after_its_contract() {
        let found = |path: &str| {
            json!({"name": "TeamToken", "file_path": path, "source_code": "contract TeamToken {}",
                   "compiler_version": "v0.6.12+commit.27d51765", "optimization_enabled": true})
        };
        let source = from_blockscout(Address::ZERO, &found(".sol")).unwrap().named();
        assert_eq!(source.fully_qualified_name, "TeamToken.sol:TeamToken");
        assert_eq!(source.sources.keys().collect::<Vec<_>>(), ["TeamToken.sol"]);
        let nested = from_blockscout(Address::ZERO, &found("flat/.sol")).unwrap().named();
        assert_eq!(nested.main_path(), "flat/TeamToken.sol");
        let fine = from_blockscout(Address::ZERO, &found("src/Token.sol")).unwrap().named();
        assert_eq!(fine.main_path(), "src/Token.sol");
    }
}
