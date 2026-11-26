//! Claims, verdicts and scoring for Kea.
//!
//! Kea answers questions about deployed contracts with proofs. A drafter turns a question into
//! [`claim::Claim`]s, each tied to a symbolic check; a prover runs the checks; counterexamples
//! are replayed on a fork of the live chain. This crate holds what those steps share: the claim
//! catalog, the reading of generated test files, and the rules that decide whether a result is
//! a proof, a finding, or neither ([`verdict::Evidence::verdict`]).
//!
//! The rules are strict on purpose. A pass that saw no symbolic input, reached no successful
//! path, or checked less than its words claim is never reported as proven, and a counterexample
//! that did not replay, or replayed only by forging state, is never reported as broken.

pub mod claim;
pub mod run;
pub mod solidity;
pub mod verdict;

pub use claim::{Claim, ClaimKind, Scope};
pub use run::{ContractRun, Reviews, Scored};
pub use verdict::Verdict;
