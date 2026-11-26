//! Reading a generated Solidity test file the way the scorer needs to.
//!
//! The scorer never parses Solidity properly; it only needs to know, for each `check_` and
//! `test_replay_` function, what text runs when that function runs. A function's text is
//! everything from its `function name(` up to the next `function`, and the text that runs is
//! that plus the bodies of the non-test helpers it calls directly.

use crate::claim::Scope;
use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

static FUNCTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfunction\s+(\w+)\s*\(").expect("static pattern"));

/// Code identity does not change inside one call, so a check about it proves nothing.
static TRIVIAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"codehash|extcodesize|\.code\.length").expect("static pattern"));

/// Calls that write storage or code no real account could: a replay using them is not evidence.
const CHEATS: [&str; 3] = ["vm.store(", "vm.etch(", "stdstore"];

/// Where symbolic inputs come from in a halmos test.
const SYMBOLIC: &str = "svm.create";

/// A test file split into functions, in source order. A name defined twice keeps its first
/// position and its last text, which is what a later overload shadowing an earlier one means
/// for the scorer.
#[derive(Clone, Debug, Default)]
pub struct TestFile {
    functions: Vec<(String, String)>,
}

impl TestFile {
    /// Splits a source file into function texts.
    pub fn parse(source: &str) -> Self {
        let heads: Vec<(usize, usize, String)> = FUNCTION
            .captures_iter(source)
            .filter_map(|caps| {
                let whole = caps.get(0)?;
                Some((whole.start(), whole.end(), caps.get(1)?.as_str().to_string()))
            })
            .collect();
        let mut functions: Vec<(String, String)> = Vec::with_capacity(heads.len());
        for (i, (_, body_start, name)) in heads.iter().enumerate() {
            let body_end = heads.get(i + 1).map_or(source.len(), |next| next.0);
            let body = source[*body_start..body_end].to_string();
            match functions.iter_mut().find(|(known, _)| known == name) {
                Some(slot) => slot.1 = body,
                None => functions.push((name.clone(), body)),
            }
        }
        Self { functions }
    }

    /// The text of one function, or an empty string when the file has no such function.
    pub fn body(&self, name: &str) -> &str {
        self.functions.iter().find(|(known, _)| known == name).map_or("", |(_, body)| body)
    }

    /// The text that runs when `name` runs: its own text, then the text of every non-test
    /// helper it calls directly, in source order.
    pub fn running_text(&self, name: &str) -> String {
        let body = self.body(name);
        let mut text = body.to_string();
        for (helper, helper_body) in &self.functions {
            let is_test = helper.starts_with("check_") || helper.starts_with("test_");
            if helper != name && !is_test && calls(body, helper) {
                text.push_str(helper_body);
            }
        }
        text
    }

    /// The `check_` functions whose run sees a symbolic input, from their own text, a helper
    /// they call, or `setUp`.
    pub fn symbolic_checks(&self) -> BTreeSet<String> {
        let set_up_is_symbolic = self.body("setUp").contains(SYMBOLIC);
        self.functions
            .iter()
            .map(|(name, _)| name)
            .filter(|name| name.starts_with("check_"))
            .filter(|name| set_up_is_symbolic || self.running_text(name).contains(SYMBOLIC))
            .cloned()
            .collect()
    }
}

/// Whether `body` calls a function named `name`.
fn calls(body: &str, name: &str) -> bool {
    Regex::new(&format!(r"\b{}\s*\(", regex::escape(name))).is_ok_and(|call| call.is_match(body))
}

/// Whether a check's text is only about code identity, which cannot change within one call.
pub fn is_trivial(text: &str) -> bool {
    TRIVIAL.is_match(text)
}

/// Whether a replay's text writes state no real account could.
pub fn uses_cheats(text: &str) -> bool {
    CHEATS.iter().any(|cheat| text.contains(cheat))
}

/// Whether a check is at least as wide as the scope its claim declares: `Any` needs symbolic
/// calldata, a named function must be called by name.
pub fn covers(scope: &Scope, text: &str) -> bool {
    match scope {
        Scope::Any => text.contains("svm.createCalldata"),
        Scope::Function(name) => {
            Regex::new(&format!(r"\b{}\b", regex::escape(name))).is_ok_and(|word| word.is_match(text))
        }
    }
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
        function _fake() internal { vm.store(LIVE, 0, 0); }\n";

    #[test]
    fn functions_split_at_each_head() {
        let file = TestFile::parse(FILE);
        assert!(file.body("check_B").contains("assert(true)"));
        assert!(!file.body("check_B").contains("pick"));
        assert_eq!(file.body("missing"), "");
    }

    #[test]
    fn running_text_follows_direct_helper_calls() {
        let file = TestFile::parse(FILE);
        assert!(file.running_text("check_C").contains("svm.createUint256('p')"));
        assert!(uses_cheats(&file.running_text("test_replay_C")));
        assert!(!uses_cheats(file.body("test_replay_C")));
    }

    #[test]
    fn symbolic_checks_see_helpers_and_set_up() {
        let file = TestFile::parse(FILE);
        let found: Vec<_> = file.symbolic_checks().into_iter().collect();
        assert_eq!(found, ["check_A", "check_C"]);
        let with_symbolic_set_up =
            TestFile::parse("function setUp() public { a = svm.createAddress('a'); }\nfunction check_X() public {}\n");
        assert!(with_symbolic_set_up.symbolic_checks().contains("check_X"));
    }

    #[test]
    fn a_later_overload_keeps_the_first_position() {
        let file = TestFile::parse("function a() {one}\nfunction b() {two}\nfunction a(uint x) {three}\n");
        assert!(file.body("a").contains("three"));
        assert_eq!(file.functions.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn coverage_matches_the_declared_scope() {
        assert!(covers(&Scope::Any, "bytes memory d = svm.createCalldata('X');"));
        assert!(!covers(&Scope::Any, "t.claimBond();"));
        assert!(covers(&Scope::Function("claimBond".into()), "t.claimBond();"));
        assert!(!covers(&Scope::Function("claim".into()), "t.claimBond();"));
        assert!(covers(&Scope::Function(String::new()), "t.x();"));
        assert!(is_trivial("assert(address(t).codehash == h);"));
        assert!(!is_trivial("assert(t.fee() <= 10);"));
    }
}
