//! The expected set, from Solami's `getTransactionsForAddress`.
//!
//! With `filters: { slot: { gte, lte }, status: "succeeded" }` it returns every successful
//! transaction that touches the address, including through an address lookup table. That's the
//! same rule as the stream's `account_include` filter with `failed: false`. Phase 0 checked it
//! against full blocks: 174 of 174, nothing missing or extra, at ~1% of the bytes.

use std::collections::HashMap;

use futures::StreamExt;
use gapless::{Signature, SlotRange};
use serde_json::{Value, json};

use crate::Error;
use crate::rpc::Rpc;

const PAGE_LIMIT: u64 = 1_000;
const MAX_PAGES: usize = 500;

/// Where a transaction landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Landed {
    pub slot: u64,
    pub index: u64,
}

#[derive(Debug, Default)]
pub struct Page {
    pub rows: Vec<(Signature, Landed)>,
    pub next: Option<String>,
}

pub fn parse_page(result: &Value) -> Result<Page, Error> {
    let data = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Unexpected("getTransactionsForAddress: no data array".into()))?;
    let mut rows = Vec::with_capacity(data.len());
    for row in data {
        let sig = row
            .get("signature")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let signature = decode_signature(sig)?;
        let slot = row
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Unexpected(format!("row without a slot: {row}")))?;
        let index = row
            .get("transactionIndex")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        rows.push((signature, Landed { slot, index }));
    }
    let next = result
        .get("paginationToken")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(Page { rows, next })
}

pub fn decode_signature(b58: &str) -> Result<Signature, Error> {
    bs58::decode(b58)
        .into_vec()
        .ok()
        .and_then(|bytes| Signature::from_bytes(&bytes))
        .ok_or_else(|| Error::Unexpected(format!("not a signature: {b58:?}")))
}

/// Every matching transaction for one address in one slot range, following pagination.
async fn one_range(
    rpc: &Rpc,
    address: &str,
    range: SlotRange,
    include_failed: bool,
) -> Result<Vec<(Signature, Landed)>, Error> {
    let mut out = Vec::new();
    let mut token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut filters = json!({ "slot": { "gte": range.first, "lte": range.last } });
        if !include_failed {
            filters["status"] = json!("succeeded");
        }
        // Default (descending) order. Ascending starts from the deep archive.
        let mut config =
            json!({ "transactionDetails": "signatures", "limit": PAGE_LIMIT, "filters": filters });
        if let Some(t) = &token {
            config["paginationToken"] = json!(t);
        }
        let result = rpc
            .call("getTransactionsForAddress", json!([address, config]))
            .await?;
        let page = parse_page(&result)?;
        let empty = page.rows.is_empty();
        out.extend(
            page.rows
                .into_iter()
                .filter(|(_, l)| range.contains(l.slot)),
        );
        // Pages can come back short before the end, so only an empty page or a missing or
        // repeated token means we're done.
        match page.next {
            Some(next) if !empty && token.as_deref() != Some(next.as_str()) => token = Some(next),
            _ => return Ok(out),
        }
    }
    Err(Error::Unexpected(format!(
        "more than {MAX_PAGES} pages for {address} in {range:?}"
    )))
}

/// The expected set for `range`: every successful transaction touching any of `addresses`.
/// The range is split into chunks fetched in parallel.
pub async fn expected(
    rpc: &Rpc,
    addresses: &[String],
    range: SlotRange,
    include_failed: bool,
    chunk_slots: u64,
    concurrency: usize,
) -> Result<HashMap<Signature, Landed>, Error> {
    let mut jobs = Vec::new();
    for address in addresses {
        let mut first = range.first;
        while first <= range.last {
            let last = (first + chunk_slots - 1).min(range.last);
            jobs.push((address.clone(), SlotRange::new(first, last)));
            first = last + 1;
        }
    }
    let results: Vec<Result<Vec<(Signature, Landed)>, Error>> = futures::stream::iter(jobs)
        .map(
            |(address, chunk)| async move { one_range(rpc, &address, chunk, include_failed).await },
        )
        .buffer_unordered(concurrency)
        .collect()
        .await;
    let mut set = HashMap::new();
    for rows in results {
        set.extend(rows?);
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIG: &str =
        "3nqt1xMRi5dTAezqDzJBmSULKbtXqxUHCgky8qRZcmzFxzvWF6gLsA7j1UjW61ig3Lg3mPYb1biswu7Z6KSBu74W";

    #[test]
    fn parses_a_page() {
        let result = json!({
            "data": [{ "signature": SIG, "slot": 450530506, "transactionIndex": 12, "err": null,
                       "memo": null, "blockTime": 1790390000, "confirmationStatus": "finalized" }],
            "paginationToken": "450530506:12"
        });
        let page = parse_page(&result).unwrap();
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].0.to_string(), SIG);
        assert_eq!(
            page.rows[0].1,
            Landed {
                slot: 450530506,
                index: 12
            }
        );
        assert_eq!(page.next.as_deref(), Some("450530506:12"));
    }

    #[test]
    fn rejects_rows_without_a_valid_signature() {
        let result = json!({ "data": [{ "signature": "not-base58!", "slot": 1 }] });
        assert!(parse_page(&result).is_err());
    }
}
