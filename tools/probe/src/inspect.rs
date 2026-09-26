use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use anyhow::{Result, bail};
use prost::Message;
use solami::geyser::subscribe_update::UpdateOneof;
use solami::geyser::{SlotStatus, SubscribeUpdate};

/// Reads a capture written by `record`: frames of
/// `[u64 LE receive-time nanos][length-delimited SubscribeUpdate]`.
pub fn frames(bytes: &[u8]) -> Result<Vec<(u64, SubscribeUpdate)>> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        if rest.len() < 8 {
            bail!(
                "truncated frame header at byte {}",
                bytes.len() - rest.len()
            );
        }
        let at = u64::from_le_bytes(rest[..8].try_into()?);
        rest = &rest[8..];
        let update = SubscribeUpdate::decode_length_delimited(&mut rest)?;
        out.push((at, update));
    }
    Ok(out)
}

/// Summarize a capture: counts, time span, slot coverage and ordering.
pub fn inspect(path: &Path) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let frames = frames(&bytes)?;
    let (Some(first), Some(last)) = (frames.first(), frames.last()) else {
        bail!("{} has no frames", path.display());
    };
    let span = (last.0 - first.0) as f64 / 1e9;

    let mut txs = 0u64;
    let mut sigs = HashSet::new();
    let mut statuses: BTreeMap<String, u64> = BTreeMap::new();
    let mut processed = HashSet::new();
    let (mut late, mut min_slot, mut max_slot) = (0u64, u64::MAX, 0u64);
    for (_, u) in &frames {
        match &u.update_oneof {
            Some(UpdateOneof::Transaction(tx)) => {
                txs += 1;
                if let Some(info) = &tx.transaction {
                    sigs.insert(info.signature.clone());
                }
                if processed.contains(&tx.slot) {
                    late += 1;
                }
                min_slot = min_slot.min(tx.slot);
                max_slot = max_slot.max(tx.slot);
            }
            Some(UpdateOneof::Slot(s)) => {
                if s.status() == SlotStatus::SlotProcessed {
                    processed.insert(s.slot);
                }
                *statuses.entry(format!("{:?}", s.status())).or_default() += 1;
            }
            _ => {}
        }
    }
    println!(
        "{}: {:.1} MB, {} frames over {span:.0}s",
        path.display(),
        bytes.len() as f64 / 1e6,
        frames.len()
    );
    println!(
        "transactions: {txs} ({:.1}/s), unique {}",
        txs as f64 / span.max(1.0),
        sigs.len()
    );
    println!(
        "tx slots {min_slot}..={max_slot} ({} slots)",
        max_slot.saturating_sub(min_slot) + 1
    );
    println!("slot updates: {statuses:?}");
    println!("transactions after their slot's SlotProcessed: {late}");
    Ok(())
}
