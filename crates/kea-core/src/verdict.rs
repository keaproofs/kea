//! What a claim's evidence amounts to.
//!
//! A symbolic pass is a proof only when the check saw a symbolic input, reached at least one
//! path that did not revert, was not about code identity, and was at least as wide as the
//! claim's words. A counterexample becomes a finding only when it replayed on a fork of the
//! live chain without forging state, and, once holders are resolved, only when the power is
//! held by an account a person controls.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The verdict on one claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Proven for every input the check covers.
    Proven,
    /// Every path reverted or was assumed away: nothing was checked.
    Vacuous,
    /// The check ran on one fixed example, which is not a proof of "always".
    Concrete,
    /// The check is about code identity, which cannot change within one call.
    Trivial,
    /// The words claim more than the check covers.
    Narrow,
    /// A reviewer found that the check does not test what the words say.
    Mismatch,
    /// A power, replayed on the live chain, held by an account a person controls.
    Finding,
    /// A counterexample that did not replay: held back, never shown as broken.
    FindingUnreplayed,
    /// The replay wrote storage or code no real account could.
    FindingCheat,
    /// The power exists in code but nobody holds it any more.
    PowerRenounced,
    /// The holder of the power could not be resolved.
    FindingNoholder,
    /// The power is held by another contract, whose own rules decide who can use it.
    ContractHeld,
    /// The prover gave no usable answer.
    Unknown,
}

impl Verdict {
    /// Every verdict, in the order above.
    pub const ALL: [Verdict; 13] = [
        Self::Proven,
        Self::Vacuous,
        Self::Concrete,
        Self::Trivial,
        Self::Narrow,
        Self::Mismatch,
        Self::Finding,
        Self::FindingUnreplayed,
        Self::FindingCheat,
        Self::PowerRenounced,
        Self::FindingNoholder,
        Self::ContractHeld,
        Self::Unknown,
    ];

    /// The name used in result files and reviews, such as `"finding-unreplayed"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Proven => "proven",
            Self::Vacuous => "vacuous",
            Self::Concrete => "concrete",
            Self::Trivial => "trivial",
            Self::Narrow => "narrow",
            Self::Mismatch => "mismatch",
            Self::Finding => "finding",
            Self::FindingUnreplayed => "finding-unreplayed",
            Self::FindingCheat => "finding-cheat",
            Self::PowerRenounced => "power-renounced",
            Self::FindingNoholder => "finding-noholder",
            Self::ContractHeld => "contract-held",
            Self::Unknown => "unknown",
        }
    }

    /// The verdict with this name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|verdict| verdict.name() == name)
    }

    /// A pass that is not a proof.
    pub fn is_weak(self) -> bool {
        matches!(self, Self::Concrete | Self::Vacuous | Self::Trivial | Self::Mismatch | Self::Narrow)
    }

    /// A counterexample dropped because it is not evidence of a power a person holds.
    pub fn is_dropped(self) -> bool {
        matches!(self, Self::FindingCheat | Self::ContractHeld | Self::FindingNoholder)
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What the symbolic run said about one check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every path the prover explored satisfied the assertion.
    Pass,
    /// The prover found an input that breaks the assertion.
    Counterexample,
    /// Anything else: timeout, error, or no run.
    Other,
}

/// Who holds a replayed power, as resolved on the live chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// Holders were never resolved for this run.
    NotResolved,
    /// A plain account a person controls.
    Account,
    /// A Safe multisig (the kind names its threshold, such as `"Safe 2"`).
    Safe,
    /// Nobody: the power was renounced.
    Nobody,
    /// The holder could not be read.
    Unresolved,
    /// Another contract.
    Contract,
}

impl Holder {
    /// Reads a holder kind written by the replay step: `"EOA"`, `"Safe …"`, `"none"`, `""`,
    /// `"unknown"`, or anything else for a contract.
    pub fn from_kind(kind: &str) -> Self {
        match kind {
            "EOA" => Self::Account,
            "none" => Self::Nobody,
            // "unused": an address that never sent a transaction holds nothing a person can use.
            "" | "unknown" | "unused" => Self::Unresolved,
            other if other.starts_with("Safe") => Self::Safe,
            _ => Self::Contract,
        }
    }
}

/// How a check's reach compares with the scope its claim declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// The run's claims declare no scopes (older runs), so reach is not judged.
    Undeclared,
    /// The check covers the declared scope.
    Covers,
    /// The check covers less than the words claim.
    Narrower,
}

impl Coverage {
    /// Coverage from whether the run declares scopes and whether this check covers its own.
    pub fn of(scopes_declared: bool, covers_scope: bool) -> Self {
        match (scopes_declared, covers_scope) {
            (false, _) => Self::Undeclared,
            (true, true) => Self::Covers,
            (true, false) => Self::Narrower,
        }
    }
}

/// What the fork replay of a counterexample showed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replay {
    /// No replay, or a replay that did not end in `Success`.
    Missing,
    /// It replayed, but only by writing state no real account could.
    Forged,
    /// It replayed through real calls only.
    Clean,
}

impl Replay {
    /// The replay state from its status and whether its text forges state. A replay that did
    /// not succeed is missing whatever its text says.
    pub fn of(succeeded: bool, forges_state: bool) -> Self {
        match (succeeded, forges_state) {
            (false, _) => Self::Missing,
            (true, true) => Self::Forged,
            (true, false) => Self::Clean,
        }
    }
}

/// Everything the verdict on one claim depends on.
#[derive(Clone, Debug)]
pub struct Evidence<'a> {
    /// What the symbolic run said.
    pub outcome: Outcome,
    /// Paths that reached the end of the check without reverting.
    pub successful_paths: u64,
    /// Whether the check saw a symbolic input.
    pub symbolic: bool,
    /// The text that runs when the check runs.
    pub check_text: &'a str,
    /// How the check's reach compares with the claim's words.
    pub coverage: Coverage,
    /// What the fork replay showed.
    pub replay: Replay,
    /// Who holds the replayed power.
    pub holder: Holder,
}

impl Evidence<'_> {
    /// The verdict, decided in a fixed order so that the first disqualifying fact wins.
    pub fn verdict(&self) -> Verdict {
        match self.outcome {
            Outcome::Pass if self.successful_paths == 0 => Verdict::Vacuous,
            Outcome::Pass if !self.symbolic => Verdict::Concrete,
            Outcome::Pass if crate::solidity::is_trivial(self.check_text) => Verdict::Trivial,
            Outcome::Pass if self.coverage == Coverage::Narrower => Verdict::Narrow,
            Outcome::Pass => Verdict::Proven,
            Outcome::Counterexample if self.replay == Replay::Missing => Verdict::FindingUnreplayed,
            Outcome::Counterexample if self.replay == Replay::Forged => Verdict::FindingCheat,
            Outcome::Counterexample => match self.holder {
                Holder::NotResolved | Holder::Account | Holder::Safe => Verdict::Finding,
                Holder::Nobody => Verdict::PowerRenounced,
                Holder::Unresolved => Verdict::FindingNoholder,
                Holder::Contract => Verdict::ContractHeld,
            },
            Outcome::Other => Verdict::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass() -> Evidence<'static> {
        Evidence {
            outcome: Outcome::Pass,
            successful_paths: 2,
            symbolic: true,
            check_text: "assert(t.fee() <= 10);",
            coverage: Coverage::Covers,
            replay: Replay::Missing,
            holder: Holder::NotResolved,
        }
    }

    #[test]
    fn passes_must_earn_the_word_proven() {
        assert_eq!(pass().verdict(), Verdict::Proven);
        assert_eq!(Evidence { successful_paths: 0, ..pass() }.verdict(), Verdict::Vacuous);
        assert_eq!(Evidence { symbolic: false, ..pass() }.verdict(), Verdict::Concrete);
        assert_eq!(Evidence { check_text: "x.codehash", ..pass() }.verdict(), Verdict::Trivial);
        assert_eq!(Evidence { coverage: Coverage::Narrower, ..pass() }.verdict(), Verdict::Narrow);
        assert_eq!(Evidence { coverage: Coverage::Undeclared, ..pass() }.verdict(), Verdict::Proven);
        assert_eq!(Coverage::of(false, false), Coverage::Undeclared);
        assert_eq!(Coverage::of(true, false), Coverage::Narrower);
    }

    #[test]
    fn counterexamples_must_replay_and_have_a_holder() {
        let cex = Evidence { outcome: Outcome::Counterexample, replay: Replay::Clean, ..pass() };
        assert_eq!(Evidence { replay: Replay::Missing, ..cex.clone() }.verdict(), Verdict::FindingUnreplayed);
        assert_eq!(Evidence { replay: Replay::Forged, ..cex.clone() }.verdict(), Verdict::FindingCheat);
        assert_eq!(Replay::of(false, true), Replay::Missing);
        assert_eq!(cex.verdict(), Verdict::Finding);
        for (kind, verdict) in [
            ("EOA", Verdict::Finding),
            ("Safe 2", Verdict::Finding),
            ("none", Verdict::PowerRenounced),
            ("", Verdict::FindingNoholder),
            ("unknown", Verdict::FindingNoholder),
            ("unused", Verdict::FindingNoholder),
            ("contract", Verdict::ContractHeld),
        ] {
            let resolved = Evidence { holder: Holder::from_kind(kind), ..cex.clone() };
            assert_eq!(resolved.verdict(), verdict, "holder kind {kind:?}");
        }
        assert_eq!(Evidence { outcome: Outcome::Other, ..pass() }.verdict(), Verdict::Unknown);
    }

    #[test]
    fn names_round_trip() {
        for verdict in Verdict::ALL {
            assert_eq!(Verdict::from_name(verdict.name()), Some(verdict));
            let json = serde_json::to_string(&verdict).unwrap();
            assert_eq!(json, format!("\"{}\"", verdict.name()));
        }
    }
}
