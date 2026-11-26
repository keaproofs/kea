//! EIP-1967 proxies: the code that runs lives at the implementation address.

use alloy_primitives::{Address, B256, b256};
use kea_chain::{Rpc, RpcError};

/// `bytes32(uint256(keccak256("eip1967.proxy.implementation")) - 1)`.
pub const IMPLEMENTATION_SLOT: B256 = b256!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc");
/// `bytes32(uint256(keccak256("eip1967.proxy.admin")) - 1)`.
pub const ADMIN_SLOT: B256 = b256!("b53127684a568b3173ae13b9f8a6016e243e63b6e8ee1178d6a717850b5d6103");

/// What the proxy slots of an address say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proxy {
    /// The implementation whose code runs.
    pub implementation: Address,
    /// The admin recorded in the admin slot; zero for proxies that keep it elsewhere.
    pub admin: Address,
}

/// Reads the EIP-1967 slots of `address`. `None` when the implementation slot is empty.
///
/// # Errors
/// When the chain cannot be read.
pub async fn resolve(rpc: &Rpc, address: Address) -> Result<Option<Proxy>, RpcError> {
    let implementation = word_address(rpc.storage_at(address, IMPLEMENTATION_SLOT).await?);
    if implementation.is_zero() {
        return Ok(None);
    }
    let admin = word_address(rpc.storage_at(address, ADMIN_SLOT).await?);
    Ok(Some(Proxy { implementation, admin }))
}

/// The address in the low 20 bytes of a storage word.
fn word_address(word: B256) -> Address {
    Address::from_slice(&word[12..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{U256, keccak256};

    fn eip1967(label: &str) -> B256 {
        B256::from(U256::from_be_bytes(keccak256(label).0) - U256::from(1))
    }

    #[test]
    fn slots_are_derived_not_typed() {
        assert_eq!(IMPLEMENTATION_SLOT, eip1967("eip1967.proxy.implementation"));
        assert_eq!(ADMIN_SLOT, eip1967("eip1967.proxy.admin"));
    }

    #[test]
    fn the_address_is_the_low_twenty_bytes() {
        let mut word = [0u8; 32];
        word[12..].copy_from_slice(&[0x4e; 20]);
        assert_eq!(word_address(B256::from(word)), Address::from([0x4e; 20]));
    }
}
