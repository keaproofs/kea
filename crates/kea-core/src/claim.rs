//! The claim catalog and the claim record a drafter produces.
//!
//! A claim is a plain-word statement about a contract ("nobody can mint new tokens") tied to the
//! symbolic check that decides it. Its id starts with a catalog code (`T1_mint`, `V1_withdraw`);
//! a claim whose id carries no known code is kept, but never counts towards a contract's score.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The family of contract a claim kind belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// ERC-20 tokens and their variants.
    Token,
    /// Upgradeable contracts and proxies.
    Upgradeable,
    /// Vaults, banks, staking, locks, vesting and escrow.
    Vault,
    /// Pools, routers, hooks, launch curves and sales.
    Market,
    /// Payouts, fee splitters, rewards and distributors.
    Payout,
    /// Any contract with an owner or admin.
    Admin,
}

/// What a claim is about. The variant name is the code that prefixes a claim id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClaimKind {
    /// Nobody can create new tokens after deployment or launch.
    T1,
    /// Nobody but the holder, or someone the holder approved, can lower a holder's balance.
    T2,
    /// A holder can always transfer their tokens: no pause, blacklist, max-tx or cooldown stops it.
    T3,
    /// The fee on a transfer or trade never exceeds N%, and nobody can raise it above N%.
    T4,
    /// Nobody can change balances arbitrarily (rebase, reflection, rescue functions).
    T5,
    /// Who can replace the contract's code, or a proof that nobody can.
    P1,
    /// `initialize()` can run only once: a second call reverts for every caller.
    P2,
    /// A depositor can always withdraw what they are owed.
    V1,
    /// Nobody can move depositors' funds to themselves.
    V2,
    /// Locked or vesting tokens cannot leave before their unlock time, for any caller.
    V3,
    /// Total user claims never exceed the assets the contract holds.
    V4,
    /// The fee on a swap never exceeds N%, and nobody can raise it above N%.
    H1,
    /// Nobody can stop a user from swapping or selling.
    H2,
    /// A swap or buy cannot take more of the user's tokens than the quote plus the fee.
    H3,
    /// Nobody can withdraw the assets backing a curve or sale except by the documented rule.
    C1,
    /// Payout shares or recipients cannot be changed.
    F1,
    /// A recipient can always claim what is owed to them.
    F2,
    /// An owner-only function that moves money or changes the rules, or a proof that there is none.
    O1,
}

impl ClaimKind {
    /// Every kind in catalog order.
    pub const ALL: [ClaimKind; 18] = [
        Self::T1,
        Self::T2,
        Self::T3,
        Self::T4,
        Self::T5,
        Self::P1,
        Self::P2,
        Self::V1,
        Self::V2,
        Self::V3,
        Self::V4,
        Self::H1,
        Self::H2,
        Self::H3,
        Self::C1,
        Self::F1,
        Self::F2,
        Self::O1,
    ];

    /// The catalog code, such as `"T1"`.
    pub fn code(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::T3 => "T3",
            Self::T4 => "T4",
            Self::T5 => "T5",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::V1 => "V1",
            Self::V2 => "V2",
            Self::V3 => "V3",
            Self::V4 => "V4",
            Self::H1 => "H1",
            Self::H2 => "H2",
            Self::H3 => "H3",
            Self::C1 => "C1",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::O1 => "O1",
        }
    }

    /// The kind with this exact code.
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }

    /// The kind named by a claim id: the code, then an underscore (`"V1_withdraw"` is `V1`).
    pub fn of_id(id: &str) -> Option<Self> {
        id.split_once('_').and_then(|(code, _)| Self::from_code(code))
    }

    /// The family of contract this kind is written for.
    pub fn family(self) -> Family {
        match self {
            Self::T1 | Self::T2 | Self::T3 | Self::T4 | Self::T5 => Family::Token,
            Self::P1 | Self::P2 => Family::Upgradeable,
            Self::V1 | Self::V2 | Self::V3 | Self::V4 => Family::Vault,
            Self::H1 | Self::H2 | Self::H3 | Self::C1 => Family::Market,
            Self::F1 | Self::F2 => Family::Payout,
            Self::O1 => Family::Admin,
        }
    }
}

impl fmt::Display for ClaimKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Whether a claim is expected to hold, or names a power someone has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Expect {
    /// The claim should be proven for every input.
    Hold,
    /// The claim names a power; the check looks for the transaction that uses it.
    Power,
    /// Any other value a drafter wrote.
    #[serde(other)]
    Other,
}

/// What a claim's words cover: any function of the contract, or one named function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Any call: the check must drive the contract with symbolic calldata.
    Any,
    /// Only this function: the check must call it.
    Function(String),
}

impl Scope {
    /// Reads a declared scope. A missing or empty scope means `Any`, and a signature such as
    /// `transfer(address,uint256)` names its function. Whitespace is trimmed only after the
    /// missing/empty test, as the published scorer does, so a blank scope names no function.
    pub fn parse(declared: Option<&str>) -> Self {
        let text = match declared {
            Some(text) if !text.is_empty() => text.trim(),
            _ => "any",
        };
        let name = text.split('(').next().unwrap_or(text);
        if name == "any" { Self::Any } else { Self::Function(name.to_string()) }
    }
}

/// A claim as a drafter writes it: the words, the check that decides it, and what it covers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// Catalog code, an underscore and a short name: `V1_withdraw`.
    pub id: String,
    /// The plain words a trader reads.
    pub plain: String,
    /// Whether the claim should hold or names a power.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect: Option<Expect>,
    /// The `check_` function whose symbolic run decides the claim.
    pub check: String,
    /// The `test_replay_` function that replays a counterexample on a fork of the live chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<String>,
    /// What the words cover; see [`Scope::parse`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Claim {
    /// The catalog kind named by the id, if any.
    pub fn kind(&self) -> Option<ClaimKind> {
        ClaimKind::of_id(&self.id)
    }

    /// Whether the claim counts towards a contract's score: only catalog kinds do.
    pub fn meaningful(&self) -> bool {
        self.kind().is_some()
    }

    /// What the words cover.
    pub fn scope(&self) -> Scope {
        Scope::parse(self.scope.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_name_their_kind() {
        assert_eq!(ClaimKind::of_id("V1_withdraw"), Some(ClaimKind::V1));
        assert_eq!(ClaimKind::of_id("O1_"), Some(ClaimKind::O1));
        assert_eq!(ClaimKind::of_id("T12_supply"), None);
        assert_eq!(ClaimKind::of_id("X1_other"), None);
        assert_eq!(ClaimKind::of_id("T1"), None);
        for kind in ClaimKind::ALL {
            assert_eq!(ClaimKind::from_code(kind.code()), Some(kind));
        }
    }

    #[test]
    fn scopes_follow_the_declared_words() {
        assert_eq!(Scope::parse(None), Scope::Any);
        assert_eq!(Scope::parse(Some("")), Scope::Any);
        assert_eq!(Scope::parse(Some("  ")), Scope::Function(String::new()));
        assert_eq!(Scope::parse(Some(" any ")), Scope::Any);
        assert_eq!(Scope::parse(Some("transfer(address,uint256)")), Scope::Function("transfer".into()));
        assert_eq!(Scope::parse(Some("Any")), Scope::Function("Any".into()));
    }

    #[test]
    fn claims_read_from_drafter_json() {
        let claim: Claim = serde_json::from_str(
            r#"{"id":"P2_initpool","plain":"An existing pool cannot be initialized a second time",
                "expect":"hold","check":"check_P2_initpool","scope":"initializePool"}"#,
        )
        .unwrap();
        assert_eq!(claim.kind(), Some(ClaimKind::P2));
        assert_eq!(claim.expect, Some(Expect::Hold));
        assert_eq!(claim.scope(), Scope::Function("initializePool".into()));
        assert_eq!(claim.kind().map(ClaimKind::family), Some(Family::Upgradeable));
    }
}
