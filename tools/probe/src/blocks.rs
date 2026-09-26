use std::time::{Duration, Instant};

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;

use crate::env::Env;
use crate::rpc::{Rpc, RpcError};

struct Fetched {
    elapsed: Duration,
    retries: u32,
    result: Result<(Value, usize), RpcError>,
}

async fn fetch(rpc: &Rpc, slot: u64) -> Fetched {
    let mut retries = 0;
    let mut delay = Duration::from_millis(100);
    loop {
        let t = Instant::now();
        let result = rpc.get_block(slot).await;
        if matches!(result, Err(RpcError::RateLimited)) && retries < 6 {
            retries += 1;
            // Cheap jitter without an RNG: spread retries by slot number.
            tokio::time::sleep(delay + Duration::from_millis(slot % 50)).await;
            delay *= 2;
            continue;
        }
        return Fetched {
            elapsed: t.elapsed(),
            retries,
            result,
        };
    }
}

fn has(list: &Value, program: &str) -> bool {
    list.as_array()
        .is_some_and(|keys| keys.iter().any(|k| k.as_str() == Some(program)))
}

/// Is `program` loaded through an address lookup table rather than listed statically?
fn via_lookup(tx: &Value, program: &str) -> bool {
    let loaded = &tx["meta"]["loadedAddresses"];
    has(&loaded["writable"], program) || has(&loaded["readonly"], program)
}

/// Does this transaction match our stream filter: successful, and `program` among its
/// account keys (static or loaded from a lookup table)?
fn matches(tx: &Value, program: &str) -> bool {
    tx["meta"]["err"].is_null()
        && (has(&tx["transaction"]["message"]["accountKeys"], program) || via_lookup(tx, program))
}

/// Spike 5: how long does it take to fetch a window of confirmed blocks within our rate limit?
pub async fn blocks(env: &Env, count: u64, concurrency: usize) -> Result<()> {
    let rpc = Rpc::new(env);
    let finalized = rpc.get_slot("finalized").await?;
    let end = finalized;
    let start = end + 1 - count;
    println!("fetching {count} blocks {start}..={end} (finalized tip), concurrency {concurrency}");

    let t0 = Instant::now();
    let results: Vec<(u64, Fetched)> = futures::stream::iter(start..=end)
        .map(|slot| {
            let rpc = rpc.clone();
            async move { (slot, fetch(&rpc, slot).await) }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;
    let wall = t0.elapsed();

    let (mut ok, mut skipped, mut limited, mut errors, mut retries) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut bytes, mut txs, mut matched, mut lookup_hits) = (0usize, 0usize, 0usize, 0usize);
    let mut lat = Vec::new();
    for (slot, f) in &results {
        retries += f.retries as u64;
        match &f.result {
            Ok((block, size)) => {
                ok += 1;
                bytes += size;
                lat.push(f.elapsed.as_secs_f64() * 1e3);
                let list = block["transactions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                txs += list.len();
                for tx in &list {
                    if matches(tx, &env.program) {
                        matched += 1;
                        if !has(&tx["transaction"]["message"]["accountKeys"], &env.program)
                            && via_lookup(tx, &env.program)
                        {
                            lookup_hits += 1;
                        }
                    }
                }
            }
            Err(RpcError::NoBlock { code, .. }) => {
                skipped += 1;
                if skipped <= 3 {
                    println!("  slot {slot}: no block ({code})");
                }
            }
            Err(RpcError::RateLimited) => limited += 1,
            Err(e) => {
                errors += 1;
                if errors <= 3 {
                    println!("  slot {slot}: {e}");
                }
            }
        }
    }
    lat.sort_by(|a, b| a.total_cmp(b));
    let p = |q: f64| {
        lat.get(((lat.len().max(1) - 1) as f64 * q).round() as usize)
            .copied()
            .unwrap_or(f64::NAN)
    };

    println!("\n-- summary");
    println!(
        "wall time: {:.2}s ({:.1} blocks/s)",
        wall.as_secs_f64(),
        results.len() as f64 / wall.as_secs_f64()
    );
    println!(
        "blocks ok {ok}, skipped {skipped}, still rate-limited {limited}, errors {errors}, retries {retries}"
    );
    println!(
        "payload: {:.1} MB total, {:.0} KB per block",
        bytes as f64 / 1e6,
        bytes as f64 / 1e3 / ok.max(1) as f64
    );
    println!(
        "per-request ms: p50 {:.0}  p90 {:.0}  p99 {:.0}",
        p(0.5),
        p(0.9),
        p(0.99)
    );
    println!(
        "transactions: {txs}; matching {} filter: {matched} ({lookup_hits} only via lookup tables)",
        env.program
    );
    Ok(())
}
