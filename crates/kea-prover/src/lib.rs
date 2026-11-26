//! Build, prove and replay: the steps that turn a drafted test file into evidence.
//!
//! Every step runs a tool under a hard cap ([`process`]). The prover decides each claim's check
//! ([`halmos`]); counterexamples are replayed on a fork of the live chain ([`forge`]); and the
//! account each replay names as a power's holder is read on the live chain ([`resolve_holders`]).

pub mod checksum;
pub mod forge;
pub mod halmos;
pub mod process;

use alloy_primitives::Address;
use kea_chain::{Rpc, account_kind};
use kea_core::run::HolderRecord;
use std::collections::BTreeMap;

/// Reads, for every replay that logged a holder, who stands behind that account.
pub async fn resolve_holders(rpc: &Rpc, holders: &BTreeMap<String, Address>) -> BTreeMap<String, Option<HolderRecord>> {
    let mut resolved = BTreeMap::new();
    for (replay, address) in holders {
        let kind = account_kind(rpc, *address).await;
        resolved.insert(
            replay.clone(),
            Some(HolderRecord { address: Some(address.to_checksum(None)), kind: Some(kind.label()) }),
        );
    }
    resolved
}
