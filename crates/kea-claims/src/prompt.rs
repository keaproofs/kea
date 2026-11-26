//! What the drafter reads: the instructions, the catalog, the contract, and on retries what went
//! wrong last time.

use alloy_primitives::{Address, U256, keccak256};
use kea_chain::Rpc;
use kea_sources::Subject;
use kea_sources::verified::VerifiedSource;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

/// How to write the test file. Stable across every request, so it caches.
pub const SYSTEM: &str = include_str!("prompts/drafter_system.md");
/// The claim kinds that count.
pub const CATALOG: &str = include_str!("prompts/catalog.md");

/// Source files from well-known libraries are listed by path, not shown: the drafter knows them
/// and they would crowd out the contract's own code.
static LIBRARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)openzeppelin|solady|solmate|@uniswap|uniswap/|forge-std|permit2|v4-core|v4-periphery|prb-math|chainlink|@layerzero")
        .expect("static pattern")
});

/// How many characters of the contract's own sources go into a request.
pub const SOURCE_BUDGET: usize = 150_000;
/// How many characters of the ABI go into a request.
const ABI_BUDGET: usize = 20_000;

/// Getters read from the live contract, for the drafter to configure `setUp` like the real one.
const GETTERS: [(&str, Kind); 6] = [
    ("owner()", Kind::Address),
    ("admin()", Kind::Address),
    ("getOwner()", Kind::Address),
    ("paused()", Kind::Number),
    ("totalSupply()", Kind::Number),
    ("decimals()", Kind::Number),
];

#[derive(Clone, Copy)]
enum Kind {
    Address,
    Number,
}

/// Reads the common getters of `address`. A getter the contract does not have is left out.
pub async fn live_getters(rpc: &Rpc, address: Address) -> Map<String, Value> {
    let mut facts = Map::new();
    for (signature, kind) in GETTERS {
        let selector = &keccak256(signature.as_bytes())[..4];
        let Ok(word) = rpc.call(address, selector).await else { continue };
        if word.len() != 32 {
            continue;
        }
        let value = match kind {
            Kind::Address => Value::String(Address::from_slice(&word[12..]).to_checksum(None)),
            Kind::Number => Value::String(U256::from_be_slice(&word).to_string()),
        };
        facts.insert(signature.to_string(), value);
    }
    facts
}

/// The contract's own sources, main file first then smallest first, up to `budget` characters;
/// and the paths left out.
pub fn sources(source: &VerifiedSource, budget: usize) -> (String, Vec<String>) {
    let main = source.main_path();
    let mut own: Vec<(&String, &String)> = source.sources.iter().filter(|(path, _)| !LIBRARY.is_match(path)).collect();
    own.sort_by_key(|(path, text)| (path.as_str() != main, text.len()));
    let mut shown = Vec::new();
    let mut left_out = Vec::new();
    let mut used = 0;
    for (path, text) in own {
        if used + text.len() > budget {
            left_out.push(path.clone());
            continue;
        }
        used += text.len();
        shown.push(format!("// ===== FILE: {path}\n{text}"));
    }
    left_out.extend(source.sources.keys().filter(|path| LIBRARY.is_match(path)).cloned());
    (shown.join("\n\n"), left_out)
}

/// Everything about the subject, in one block that stays the same across retries.
pub fn subject(subject: &Subject, getters: &Map<String, Value>, kind_guess: Option<&str>) -> String {
    let source = &subject.source;
    let proxy = subject.proxy.as_ref().map_or_else(
        || "no".to_string(),
        |p| format!("yes, implementation {}, EIP-1967 admin slot {}", p.implementation, p.admin),
    );
    let abi = source.abi.as_ref().map(Value::to_string).unwrap_or_default();
    let abi: String = abi.chars().take(ABI_BUDGET).collect();
    let (shown, left_out) = sources(source, SOURCE_BUDGET);
    let left_out: Vec<&str> = left_out.iter().take(80).map(String::as_str).collect();
    format!(
        "CLAIM CATALOG\n{CATALOG}\n\nSUBJECT\nlive address: {} on Robinhood Chain (chain id 4663)\n\
         contract: {} (compiler {})\nkind guess: {}\nproxy: {proxy}\nlive ETH balance (wei): {}\nlive getters: {}\n\
         constructor arguments (ABI-encoded hex, decode them in setUp with abi.decode if you need them): {}\n\
         ABI: {abi}\n\nSOURCES (import paths are relative to the project root)\n{shown}\n\n\
         Library files present but not shown: {}\n",
        subject.address,
        source.fully_qualified_name,
        source.compiler_version,
        kind_guess.unwrap_or("unknown"),
        subject.balance,
        Value::Object(getters.clone()),
        source.constructor_args.as_deref().unwrap_or("none"),
        left_out.join(", "),
    )
}

/// The last `lines` lines of a log.
pub fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Feedback after a draft that did not compile, or that broke the drafting rules.
pub fn rejected(problems: &str, previous_file: &str) -> String {
    format!("PREVIOUS ATTEMPT FAILED. Fix the test file.\n{problems}\n\nYour previous file:\n{previous_file}")
}

/// Feedback after an answer that held no draft. `previous_file` is the last file that did run,
/// when there is one, so the next attempt can build on it.
pub fn unusable(why: &str, previous_file: Option<&str>) -> String {
    let previous = previous_file.map(|file| format!("\n\nYour last complete file:\n{file}")).unwrap_or_default();
    format!(
        "PREVIOUS ATTEMPT HELD NO DRAFT: {why}. Answer with the JSON document only, and keep the test file within \
         about 400 lines: fewer claims, decided well, beat many claims cut off.{previous}"
    )
}

/// Feedback after a draft that compiled and ran. `review_notes` are the reviewer's reasons for
/// taking `proven` away from claims whose words were wider than their checks.
pub fn ran(
    proven: usize,
    unusable: &[String],
    review_notes: &[String],
    halmos_log: &str,
    replay_log: &str,
    previous_file: &str,
) -> String {
    let unusable = if unusable.is_empty() { "none".to_string() } else { unusable.join(", ") };
    let review = if review_notes.is_empty() {
        String::new()
    } else {
        format!(
            "Review: these proven claims are worded wider than their checks. Reword them to say only what is proved, \
             or strengthen the checks:\n- {}\n",
            review_notes.join("\n- ")
        )
    };
    format!(
        "PREVIOUS ATTEMPT: {proven} claims proven. No usable result from: {unusable}. Keep the proven checks unchanged; \
         fix these or replace them with other catalog claims. Verdicts: unknown = prover error or timeout; vacuous = every \
         path reverted; concrete = no symbolic input; narrow = words or scope wider than the check; trivial = asserts \
         something always true; finding-unreplayed = counterexample whose replay failed; contract-held/finding-noholder = \
         power held by a non-multisig contract or no holder logged.\n{review}halmos output (tail):\n{}\nreplay output \
         (tail):\n{}\n\nYour previous file:\n{previous_file}",
        tail(halmos_log, 80),
        tail(replay_log, 30),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kea_sources::verified::{CompilerSettings, Origin};
    use std::collections::BTreeMap;

    fn source(files: &[(&str, usize)]) -> VerifiedSource {
        VerifiedSource {
            sources: files
                .iter()
                .map(|(path, len)| ((*path).to_string(), "x".repeat(*len)))
                .collect::<BTreeMap<_, _>>(),
            compiler_version: "0.8.22".into(),
            settings: CompilerSettings {
                optimizer_enabled: false,
                optimizer_runs: 200,
                via_ir: false,
                evm_version: None,
                remappings: vec![],
            },
            fully_qualified_name: "src/Main.sol:Main".into(),
            abi: None,
            constructor_args: None,
            deployment: None,
            origin: Origin::Sourcify,
        }
    }

    #[test]
    fn the_main_file_comes_first_and_libraries_are_only_listed() {
        let source =
            source(&[("src/Big.sol", 90), ("src/Main.sol", 50), ("src/Small.sol", 10), ("@openzeppelin/A.sol", 5)]);
        let (shown, left_out) = sources(&source, 70);
        let main_at = shown.find("src/Main.sol").unwrap();
        let small_at = shown.find("src/Small.sol").unwrap();
        assert!(main_at < small_at);
        assert!(!shown.contains("src/Big.sol"));
        assert_eq!(left_out, ["src/Big.sol", "@openzeppelin/A.sol"]);
    }

    #[test]
    fn tails_keep_the_last_lines() {
        assert_eq!(tail("a\nb\nc\nd", 2), "c\nd");
        assert_eq!(tail("a", 5), "a");
    }

    #[test]
    fn the_prompts_name_the_rules_the_scorer_enforces() {
        for rule in ["symbolic", "scope", "codehash", "log_named_address(\"holder\"", "vm.store", "QUESTION"] {
            assert!(SYSTEM.contains(rule), "drafter prompt lost {rule}");
        }
        assert!(CATALOG.contains("O1 owner"));
    }
}
