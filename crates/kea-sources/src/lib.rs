//! Turning a deployed address into a project the prover can build.
//!
//! For a proxy, the subject is its implementation: that is the code that runs. The proxy's own
//! facts (admin, balance) stay with the subject for the claims that need them.

pub mod project;
pub mod proxy;
pub mod verified;

use alloy_primitives::{Address, U256};
use kea_chain::{Rpc, RpcError};
use proxy::Proxy;
use verified::{Explorer, SourceError, VerifiedSource};

/// A contract ready to be written out and proved.
#[derive(Clone, Debug)]
pub struct Subject {
    /// The address people interact with.
    pub address: Address,
    /// The proxy slots, when the address is an EIP-1967 proxy.
    pub proxy: Option<Proxy>,
    /// The ETH the address holds, in wei.
    pub balance: U256,
    /// The verified source of the code that runs.
    pub source: VerifiedSource,
}

impl Subject {
    /// The address whose code runs: the implementation of a proxy, else the address itself.
    pub fn code_address(&self) -> Address {
        self.proxy.as_ref().map_or(self.address, |proxy| proxy.implementation)
    }
}

/// Why a subject could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum SubjectError {
    /// The chain could not be read.
    #[error(transparent)]
    Chain(#[from] RpcError),
    /// The code that runs has no verified source.
    #[error(transparent)]
    Source(#[from] SourceError),
}

/// Reads everything needed to prove claims about `address`.
///
/// # Errors
/// See [`SubjectError`].
pub async fn fetch(rpc: &Rpc, explorer: &Explorer, address: Address) -> Result<Subject, SubjectError> {
    let (proxy, balance) = tokio::try_join!(proxy::resolve(rpc, address), rpc.balance(address))?;
    let code_address = proxy.as_ref().map_or(address, |proxy| proxy.implementation);
    let source = explorer.fetch(code_address).await?;
    Ok(Subject { address, proxy, balance, source })
}
