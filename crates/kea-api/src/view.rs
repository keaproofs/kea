//! What a report looks like to the people reading it: every claim in plain words, sorted into
//! proven, found and unclear, with the reason for each.

use crate::store::Report;
use kea_core::claim::{Expect, Family};
use kea_core::run::Judged;
use kea_core::{Reviews, Scored, Verdict};
use serde_json::{Value, json};

/// The three answers a reader sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Proved for every input the check covers, and the words say no more than that.
    Proven,
    /// A power or a break, replayed on the live chain.
    Found,
    /// Not decided either way.
    Unclear,
}

impl Status {
    /// Where a verdict lands.
    pub fn of(verdict: Verdict) -> Self {
        match verdict {
            Verdict::Proven | Verdict::PowerRenounced => Self::Proven,
            Verdict::Finding => Self::Found,
            _ => Self::Unclear,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Found => "found",
            Self::Unclear => "unclear",
        }
    }
}

/// One sentence on what a verdict means, for the reader.
pub fn why(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Proven => "Proved for every caller and input the check covers.",
        Verdict::PowerRenounced => "The power exists in the code, but nobody holds it any more.",
        Verdict::Finding => "Replayed on a copy of the live chain: the power is real, and a person's account holds it.",
        Verdict::FindingUnreplayed => {
            "The prover found a counterexample that did not replay on the live chain, so it is held back."
        }
        Verdict::FindingCheat => "The counterexample needed storage writes no real account could make.",
        Verdict::FindingNoholder => "A counterexample replayed, but who holds the power could not be read.",
        Verdict::ContractHeld => "Another contract holds the power, and its own rules decide who can use it.",
        Verdict::Vacuous => "Nothing was checked: every path reverted.",
        Verdict::Concrete => "Checked on one fixed example only, which is not a proof of always.",
        Verdict::Trivial => "The check asserts something that is always true.",
        Verdict::Narrow => "The words may claim more than the check covers.",
        Verdict::Mismatch => "A review found that the words claim more than the check proves.",
        Verdict::Unknown => "The prover gave no usable answer in time.",
    }
}

/// The family name the site groups a claim under: one reader's question per family.
fn family(family: Family) -> &'static str {
    match family {
        Family::Token => "token",
        Family::Upgradeable => "upgradeable",
        Family::Vault => "vault",
        Family::Market => "market",
        Family::Payout => "payout",
        Family::Admin => "admin",
    }
}

/// Counts for a report's headline.
pub fn summary(scored: &Scored) -> Value {
    let found = scored.claims.iter().filter(|judged| Status::of(judged.verdict) == Status::Found).count();
    let proven = scored.proven();
    json!({"proven": proven, "found": found, "unclear": scored.claims.len().saturating_sub(proven + found)})
}

/// Whether a claim stays private. A counterexample to a power claim that replayed on the live
/// chain, held by an account that has acted there, a Safe, another contract or nobody, shows a
/// power the contract's own admin holds: a design fact, published as found. Every other
/// counterexample shows a protection that broke, or a "power" the drafter named that nobody real
/// holds (an address a test made up never sent a transaction), and either may be a hole anyone can
/// use. That goes to the deployer privately first, and stays out of every public view, count,
/// badge, feed and test file until then.
pub fn withheld(judged: &Judged) -> bool {
    let broken = matches!(
        judged.verdict,
        Verdict::Finding
            | Verdict::FindingUnreplayed
            | Verdict::FindingCheat
            | Verdict::FindingNoholder
            | Verdict::ContractHeld
            | Verdict::PowerRenounced
    );
    let admin_power = judged.claim.expect == Some(Expect::Power)
        && matches!(judged.verdict, Verdict::Finding | Verdict::ContractHeld | Verdict::PowerRenounced);
    broken && !admin_power
}

/// The test file as the public may see it: the check and replay of every private claim taken out,
/// so the file is always there and its absence tells nobody that something is held.
pub fn public_test(report: &Report) -> String {
    let private: Vec<String> = withheld_claims(report)
        .into_iter()
        .flat_map(|judged| [Some(judged.claim.check), judged.claim.replay])
        .flatten()
        .filter(|name| !name.is_empty())
        .collect();
    without_functions(&report.test_source, &private)
}

/// `source` with each named Solidity function removed, with the comment lines right above it.
/// Strings and comments are skipped while matching braces, so a brace inside them cannot end a body
/// early.
fn without_functions(source: &str, names: &[String]) -> String {
    let mut text = source.to_string();
    for name in names {
        let Ok(head) = regex::Regex::new(&format!(r"\bfunction\s+{}\s*\(", regex::escape(name))) else { continue };
        while let Some(found) = head.find(&text) {
            let Some(end) = body_end(&text, found.end()) else { break };
            let mut start = text[..found.start()].rfind('\n').map_or(0, |line| line + 1);
            while start > 0 {
                let previous = text[..start - 1].rfind('\n').map_or(0, |line| line + 1);
                let line = text[previous..start - 1].trim_start();
                if !(line.starts_with("//") || line.starts_with("/*") || line.starts_with('*')) {
                    break;
                }
                start = previous;
            }
            let end = if text[end..].starts_with('\n') { end + 1 } else { end };
            text.replace_range(start..end, "");
        }
    }
    text
}

/// Where the body that opens after `from` closes: just past its `}`, or past the `;` of a function
/// with no body.
fn body_end(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut depth, mut i) = (0usize, from);
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 1;
            }
            b';' if depth == 0 => return Some(i + 1),
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Every claim, scored with the review, private ones included. Only for the operator.
pub fn score_all(report: &Report) -> Scored {
    // The review entries are the engine's own; an unknown verdict cannot appear in them.
    let reviews = Reviews::from_json(&report.review).unwrap_or_default();
    report.run.score(&report.test_source, &reviews)
}

/// A report as the public may see it: private claims left out, so every count agrees with what
/// is shown.
pub fn score(report: &Report) -> Scored {
    let mut scored = score_all(report);
    scored.claims.retain(|judged| !withheld(judged));
    scored
}

/// The claims a report keeps private.
pub fn withheld_claims(report: &Report) -> Vec<Judged> {
    score_all(report).claims.into_iter().filter(withheld).collect()
}

/// The full report. `twin_of` names the address that was proved when this one only shares its code.
pub fn report(report: &Report, asked: &str, twin_of: Option<&str>) -> Value {
    let scored = score(report);
    let claims: Vec<Value> = scored
        .claims
        .iter()
        .map(|judged| {
            let claim = &judged.claim;
            let key = format!("{}:{}", report.address, claim.id);
            let note = report.review.get(&key).and_then(|entry| entry.get("note")).and_then(Value::as_str);
            let replay = claim.replay.as_deref().filter(|replay| !replay.is_empty());
            let holder = replay
                .and_then(|replay| report.run.holders.as_ref()?.get(replay)?.as_ref())
                .map(|holder| json!({"address": holder.address, "kind": holder.kind}));
            let kind = claim.kind();
            json!({
                "id": claim.id,
                "kind": kind.map(kea_core::ClaimKind::code),
                "family": kind.map(|kind| family(kind.family())),
                "plain": claim.plain,
                "scope": claim.scope.as_deref().filter(|scope| !scope.is_empty()).unwrap_or("any"),
                "status": Status::of(judged.verdict).name(),
                "verdict": judged.verdict.name(),
                "why": why(judged.verdict),
                // A note only where the review took a proof away; a cleared claim needs none.
                "note": if judged.verdict == Verdict::Mismatch || judged.verdict == Verdict::Narrow { note } else { None },
                "holder": holder,
                "check": claim.check,
                "replay": replay,
            })
        })
        .collect();
    json!({
        "address": asked,
        "proved_address": report.address,
        "twin_of": twin_of,
        "name": scored.name,
        "implementation": report.implementation,
        "code_hash": report.code_hash,
        "checked_at": report.checked_at,
        "summary": summary(&scored),
        "claims": claims,
        "drafts": scored.drafts,
        "prove_seconds": scored.prove_seconds,
        // Always offered when there is one: private checks are taken out of the file, not the file
        // out of the report, so a missing file cannot point at a held finding.
        "test": (!report.test_source.is_empty()).then(|| format!("/v1/contracts/{}/test", report.address)),
    })
}

/// The claims that tell a reader most: one power found, then meaningful proofs, shortest words first.
fn top(scored: &Scored, count: usize) -> Vec<Value> {
    let mut found: Vec<&Judged> = scored.claims.iter().filter(|j| Status::of(j.verdict) == Status::Found).collect();
    let mut proven: Vec<&Judged> =
        scored.claims.iter().filter(|j| j.verdict == Verdict::Proven && j.meaningful()).collect();
    found.sort_by_key(|j| j.claim.plain.len());
    proven.sort_by_key(|j| j.claim.plain.len());
    found
        .into_iter()
        .take(1)
        .chain(proven)
        .take(count)
        .map(|j| json!({"id": j.claim.id, "plain": j.claim.plain, "status": Status::of(j.verdict).name()}))
        .collect()
}

/// One line of the recent list, with its sharpest claims, so a list needs one request.
pub fn listed(report: &Report) -> Value {
    let scored = score(report);
    json!({
        "address": report.address,
        "name": scored.name,
        "checked_at": report.checked_at,
        "summary": summary(&scored),
        "top": top(&scored, 3),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::report as stored;

    #[test]
    fn broken_protections_stay_private_and_admin_powers_are_published() {
        let judged = |expect: &str, verdict| Judged {
            claim: serde_json::from_value(json!({"id": "V2_sweep", "plain": "x", "check": "c", "expect": expect}))
                .unwrap(),
            verdict,
        };
        assert!(withheld(&judged("hold", Verdict::Finding)), "a protection that broke");
        assert!(withheld(&judged("hold", Verdict::FindingUnreplayed)));
        assert!(withheld(&judged("other", Verdict::ContractHeld)), "unclear intent is kept private");
        assert!(!withheld(&judged("power", Verdict::Finding)), "an admin power is a design fact");
        assert!(!withheld(&judged("hold", Verdict::Proven)));
        assert!(!withheld(&judged("power", Verdict::PowerRenounced)));
        assert!(!withheld(&judged("power", Verdict::ContractHeld)), "a timelock or governor holds it");
        // A "power" nobody real holds (a made-up address never sent a transaction), or one that did not
        // replay, or replayed by writing storage, may be a hole anyone can use.
        assert!(withheld(&judged("power", Verdict::FindingNoholder)));
        assert!(withheld(&judged("power", Verdict::FindingUnreplayed)));
        assert!(withheld(&judged("power", Verdict::FindingCheat)));
    }

    #[test]
    fn the_public_test_file_leaves_out_private_checks_only() {
        let source = "contract ProofTest is Test {\n\
            /// keep me\n\
            function check_T1_supply() public { assert(t.totalSupply() == 1); }\n\
            /// the hole\n\
            // more about it\n\
            function check_V2_sweep() public {\n\
                string memory s = \"} not the end {\"; // }\n\
                /* } */ if (x) { y(); }\n\
            }\n\
            function test_replay_V2_sweep() public { vm.createSelectFork(vm.envString(\"RH_RPC\")); }\n\
            function helper() internal {}\n\
            }\n";
        let names = ["check_V2_sweep".to_string(), "test_replay_V2_sweep".to_string()];
        let public = without_functions(source, &names);
        assert!(
            !public.contains("V2_sweep") && !public.contains("the hole") && !public.contains("not the end"),
            "{public}"
        );
        assert!(
            public.contains("/// keep me\nfunction check_T1_supply()") && public.contains("function helper()"),
            "{public}"
        );
        assert!(public.trim_end().ends_with('}'), "{public}");
        assert_eq!(without_functions(source, &[]), source);
    }

    #[test]
    fn every_verdict_lands_in_one_of_three_answers() {
        for verdict in Verdict::ALL {
            let status = Status::of(verdict);
            let expected = match verdict {
                Verdict::Proven | Verdict::PowerRenounced => Status::Proven,
                Verdict::Finding => Status::Found,
                _ => Status::Unclear,
            };
            assert_eq!(status, expected, "{verdict:?}");
            assert!(why(verdict).ends_with('.'));
        }
    }

    #[test]
    fn a_reviewed_mismatch_is_unclear_and_carries_its_note() {
        let mut report = stored("0xab", "0xc0de", 1);
        report.run = serde_json::from_value(json!({
            "address": "0xab", "name": "src/V.sol:Vault",
            "claims": [{"id": "V2_sweep", "plain": "Nobody can take ETH", "expect": "hold", "check": "check_V2_sweep",
                        "replay": "", "scope": "any"}],
            "halmos": {"check_V2_sweep": {"exitcode": 0, "paths": [3, 2, 0], "models": 0, "time": [0.1, 0.1, 0.0],
                                          "verdict": "pass"}},
            "status": {"build": "0", "prove": "0"}
        }))
        .unwrap();
        report.test_source = "function check_V2_sweep() public { address c = svm.createAddress(\"c\"); \
                              bytes memory d = svm.createCalldata(\"Vault\"); vm.prank(c); (bool ok,) = address(v).call(d); \
                              assert(address(v).balance == 1); }"
            .into();
        let unreviewed = super::report(&report, "0xab", None);
        assert_eq!(unreviewed["claims"][0]["status"], "proven", "{unreviewed}");
        report.review.insert(
            "0xab:V2_sweep".into(),
            json!({"verdict": "mismatch", "finding": "mismatch", "note": "the owner is left out"}),
        );
        let reviewed = super::report(&report, "0xab", None);
        assert_eq!(reviewed["claims"][0]["status"], "unclear");
        assert_eq!(reviewed["claims"][0]["note"], "the owner is left out");
        assert_eq!(
            (reviewed["claims"][0]["kind"].as_str(), reviewed["claims"][0]["family"].as_str()),
            (Some("V2"), Some("vault"))
        );
        assert_eq!(reviewed["summary"], json!({"proven": 0, "found": 0, "unclear": 1}));
    }
}
