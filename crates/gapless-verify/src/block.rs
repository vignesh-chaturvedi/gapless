//! A second, independent expected set: apply the stream's filter to a full block ourselves.

use gapless::Signature;
use serde_json::Value;

use crate::Error;
use crate::history::decode_signature;

pub const VOTE_PROGRAM: &str = "Vote111111111111111111111111111111111111111";

/// A transaction in a block that matches the filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockMatch {
    pub signature: Signature,
    pub index: u64,
    /// The address only appears through an address lookup table.
    pub via_lookup_table: bool,
}

fn contains(list: &Value, address: &str) -> bool {
    list.as_array()
        .is_some_and(|keys| keys.iter().any(|k| k.as_str() == Some(address)))
}

/// Transactions in `block` that the stream filter should have delivered: not votes, successful
/// unless `include_failed`, and touching any of `addresses` either directly or through a lookup
/// table (Yellowstone's `account_include` covers both).
pub fn matching(
    block: &Value,
    addresses: &[String],
    include_failed: bool,
) -> Result<Vec<BlockMatch>, Error> {
    let txs = block
        .get("transactions")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Unexpected("block without a transactions array".into()))?;
    let mut out = Vec::new();
    for (index, tx) in txs.iter().enumerate() {
        let meta = &tx["meta"];
        if !include_failed && !meta["err"].is_null() {
            continue;
        }
        let static_keys = &tx["transaction"]["message"]["accountKeys"];
        let loaded = &meta["loadedAddresses"];
        let in_static = |a: &str| contains(static_keys, a);
        let in_loaded =
            |a: &str| contains(&loaded["writable"], a) || contains(&loaded["readonly"], a);
        if in_static(VOTE_PROGRAM) || in_loaded(VOTE_PROGRAM) {
            continue;
        }
        let direct = addresses.iter().any(|a| in_static(a));
        let through_table = !direct && addresses.iter().any(|a| in_loaded(a));
        if direct || through_table {
            let sig = tx["transaction"]["signatures"][0]
                .as_str()
                .unwrap_or_default();
            out.push(BlockMatch {
                signature: decode_signature(sig)?,
                index: index as u64,
                via_lookup_table: through_table,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/data/block.json")).unwrap()
    }

    #[test]
    fn matches_direct_and_lookup_table_references() {
        let found = matching(&fixture(), &[PUMP_FUN.to_owned()], false).unwrap();
        let sigs: Vec<String> = found.iter().map(|m| m.signature.to_string()).collect();
        assert_eq!(
            found.len(),
            3,
            "two direct, one through a lookup table: {sigs:?}"
        );
        assert!(sigs.contains(&"3nqt1xMRi5dTAezqDzJBmSULKbtXqxUHCgky8qRZcmzFxzvWF6gLsA7j1UjW61ig3Lg3mPYb1biswu7Z6KSBu74W".to_owned()));
        assert!(sigs.contains(&"3W7MnrFkYe5GFzrb9mToeBVBQBneRHXefthnCPDXJPDz2xoHrTt5Xbwf1GAzwEoMQFFXUwACChqYYVG4tzA3XNNV".to_owned()));
        let via_table: Vec<_> = found.iter().filter(|m| m.via_lookup_table).collect();
        assert_eq!(via_table.len(), 1);
        assert_eq!(
            via_table[0].signature.to_string(),
            "4VBxjTFVyjnRkQ116yviYH6a7aWaE9Ej5hrpkSi8AaFMMjWcSfpxczd1vExR54idfuppx668JFtU4QrrN3z5UBGW"
        );
    }

    #[test]
    fn failed_transactions_follow_the_filter() {
        let failed = "42LcWj2qGDiYvytVmTH9PQuuo4122rp4roovZukrge6Zvgg8usmJ6KEVuNJdw4DdmVtRMuNkRambaZuJGh1WBy2D";
        let without = matching(&fixture(), &[PUMP_FUN.to_owned()], false).unwrap();
        assert!(!without.iter().any(|m| m.signature.to_string() == failed));
        let with = matching(&fixture(), &[PUMP_FUN.to_owned()], true).unwrap();
        assert!(with.iter().any(|m| m.signature.to_string() == failed));
        assert_eq!(with.len(), 4);
    }

    #[test]
    fn unrelated_addresses_match_nothing() {
        let none = matching(
            &fixture(),
            &["11111111111111111111111111111112".to_owned()],
            false,
        )
        .unwrap();
        assert!(none.is_empty());
    }
}
