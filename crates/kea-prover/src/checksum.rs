//! Address literals in EIP-55 form.
//!
//! solc rejects an address literal that is not checksummed, and drafts copy addresses from RPC
//! answers, which are lower case. A literal is rewritten only when it stands alone: not the start
//! of a longer hex string, and not preceded by more hex.

use alloy_primitives::Address;
use regex::Regex;
use std::sync::LazyLock;

static HEX40: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"0x[0-9a-fA-F]{40}").expect("static pattern"));

/// `source` with every standalone 20-byte hex literal in checksummed form.
pub fn checksum_addresses(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut copied = 0;
    for found in HEX40.find_iter(source) {
        let before = found.start().checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(found.end()).copied();
        let standalone = !before.is_some_and(|b| b.is_ascii_hexdigit() || b == b'x')
            && !after.is_some_and(|b| b.is_ascii_hexdigit());
        if !standalone {
            continue;
        }
        if let Ok(address) = found.as_str().parse::<Address>() {
            out.push_str(&source[copied..found.start()]);
            out.push_str(&address.to_checksum(None));
            copied = found.end();
        }
    }
    out.push_str(&source[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_literals_are_checksummed() {
        let source = "address o = 0x9f403140bc0574d7d36ea472b82daa1bbd4ef327;";
        assert_eq!(checksum_addresses(source), "address o = 0x9F403140Bc0574D7d36eA472b82DAa1Bbd4eF327;");
    }

    #[test]
    fn longer_hex_and_checksummed_text_stay_as_they_are() {
        let word = "bytes32 s = 0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc;";
        assert_eq!(checksum_addresses(word), word);
        let done = "0x9F403140Bc0574D7d36eA472b82DAa1Bbd4eF327";
        assert_eq!(checksum_addresses(done), done);
        let glued = "a0x9f403140bc0574d7d36ea472b82daa1bbd4ef327";
        assert_eq!(checksum_addresses(glued), glued);
    }
}
