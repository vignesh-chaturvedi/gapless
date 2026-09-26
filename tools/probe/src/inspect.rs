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

const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Instruction names worth recognising, matched by their Anchor discriminator.
const CANDIDATES: &[&str] = &[
    "create",
    "create_v2",
    "buy",
    "buy_exact_sol_in",
    "sell",
    "extend_account",
    "migrate",
    "collect_creator_fee",
    "set_params",
    "initialize",
    "withdraw",
    "claim_token_incentives",
    "init_user_volume_accumulator",
    "sync_user_volume_accumulator",
    "close_user_volume_accumulator",
    "set_creator",
    "update_global_authority",
];

fn discriminator(name: &str) -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(format!("global:{name}").as_bytes());
    hash[..8].try_into().expect("8 bytes")
}

/// Count the program's instructions (top level and inner) by discriminator.
pub fn analyze(path: &Path) -> Result<()> {
    let program =
        bs58::decode(std::env::var("GAPLESS_PROGRAM").unwrap_or_else(|_| PUMP_FUN.into()))
            .into_vec()?;
    let names: BTreeMap<[u8; 8], &str> =
        CANDIDATES.iter().map(|n| (discriminator(n), *n)).collect();
    let bytes = std::fs::read(path)?;
    let frames = frames(&bytes)?;
    let mut top: BTreeMap<[u8; 8], u64> = BTreeMap::new();
    let mut inner: BTreeMap<[u8; 8], u64> = BTreeMap::new();
    let mut txs = 0u64;
    for (_, u) in &frames {
        let Some(UpdateOneof::Transaction(tx)) = &u.update_oneof else {
            continue;
        };
        let Some(info) = &tx.transaction else {
            continue;
        };
        let (Some(t), Some(meta)) = (&info.transaction, &info.meta) else {
            continue;
        };
        let Some(msg) = &t.message else { continue };
        txs += 1;
        let keys: Vec<&[u8]> = msg
            .account_keys
            .iter()
            .chain(&meta.loaded_writable_addresses)
            .chain(&meta.loaded_readonly_addresses)
            .map(Vec::as_slice)
            .collect();
        let is_program = |idx: u32| {
            keys.get(idx as usize)
                .is_some_and(|k| *k == program.as_slice())
        };
        let disc = |data: &[u8]| -> Option<[u8; 8]> { data.get(..8)?.try_into().ok() };
        for ix in &msg.instructions {
            if is_program(ix.program_id_index)
                && let Some(d) = disc(&ix.data)
            {
                *top.entry(d).or_default() += 1;
            }
        }
        for group in &meta.inner_instructions {
            for ix in &group.instructions {
                if is_program(ix.program_id_index)
                    && let Some(d) = disc(&ix.data)
                {
                    *inner.entry(d).or_default() += 1;
                }
            }
        }
    }
    println!("{txs} transactions");
    for (label, map) in [("top-level", &top), ("inner (CPI)", &inner)] {
        println!("{label} instructions of the program:");
        let mut rows: Vec<_> = map.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for (d, n) in rows {
            let name = names.get(d).copied().unwrap_or("?");
            println!(
                "  {n:>7}  {}  {name}",
                d.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
        }
    }
    Ok(())
}

/// Keep a window of the capture, drop the heavy meta fields, and write a compressed fixture.
pub fn trim(path: &Path, out: &Path, skip: u64, secs: u64) -> Result<()> {
    let bytes = std::fs::read(path)?;
    let frames = frames(&bytes)?;
    let Some(t0) = frames.first().map(|(t, _)| *t) else {
        bail!("empty capture")
    };
    let from = t0 + skip * 1_000_000_000;
    let to = from + secs * 1_000_000_000;
    let mut kept: Vec<(u64, SubscribeUpdate)> = Vec::new();
    for (t, mut u) in frames.into_iter().filter(|(t, _)| (from..to).contains(t)) {
        if let Some(UpdateOneof::Transaction(tx)) = &mut u.update_oneof
            && let Some(meta) = tx.transaction.as_mut().and_then(|i| i.meta.as_mut())
        {
            meta.log_messages.clear();
            meta.log_messages_none = true;
            meta.pre_balances.clear();
            meta.post_balances.clear();
            meta.pre_token_balances.clear();
            meta.post_token_balances.clear();
            meta.rewards.clear();
            meta.return_data = None;
            meta.return_data_none = true;
        }
        kept.push((t, u));
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    gapless::fixture::Fixture::write(out, kept.iter().map(|(t, u)| (*t, u)), 19)?;
    let size = std::fs::metadata(out)?.len();
    println!(
        "wrote {} ({} frames, {:.1} MB)",
        out.display(),
        kept.len(),
        size as f64 / 1e6
    );
    let fixture = gapless::fixture::Fixture::load(out)?;
    println!(
        "slots {:?}, {:.0}s",
        fixture.slots(),
        fixture.duration().as_secs_f64()
    );
    Ok(())
}

/// Count Anchor events (self-CPI with the `anchor:event` tag) by event discriminator, and dump
/// one example payload of each.
pub fn events(path: &Path) -> Result<()> {
    use sha2::{Digest, Sha256};
    let program = bs58::decode(PUMP_FUN).into_vec()?;
    // Anchor's EVENT_IX_TAG, little-endian.
    let tag: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
    let known = [
        "TradeEvent",
        "CreateEvent",
        "CompleteEvent",
        "SetParamsEvent",
        "CollectCreatorFeeEvent",
        "ClaimTokenIncentivesEvent",
        "InitUserVolumeAccumulatorEvent",
        "SyncUserVolumeAccumulatorEvent",
        "CloseUserVolumeAccumulatorEvent",
        "ExtendAccountEvent",
        "CompletePumpAmmMigrationEvent",
        "DistributeCreatorFeesEvent",
        "ClaimCashbackEvent",
        "CollectCreatorFeeV2Event",
        "MigrateEvent",
    ];
    let names: BTreeMap<[u8; 8], &str> = known
        .iter()
        .map(|n| {
            (
                Sha256::digest(format!("event:{n}").as_bytes())[..8]
                    .try_into()
                    .unwrap(),
                *n,
            )
        })
        .collect();
    let bytes = std::fs::read(path)?;
    let mut counts: BTreeMap<[u8; 8], (u64, Vec<u8>)> = BTreeMap::new();
    for (_, u) in frames(&bytes)? {
        let Some(UpdateOneof::Transaction(tx)) = &u.update_oneof else {
            continue;
        };
        let Some(info) = &tx.transaction else {
            continue;
        };
        let (Some(t), Some(meta)) = (&info.transaction, &info.meta) else {
            continue;
        };
        let Some(msg) = &t.message else { continue };
        let keys: Vec<&[u8]> = msg
            .account_keys
            .iter()
            .chain(&meta.loaded_writable_addresses)
            .chain(&meta.loaded_readonly_addresses)
            .map(Vec::as_slice)
            .collect();
        for group in &meta.inner_instructions {
            for ix in &group.instructions {
                let own = keys
                    .get(ix.program_id_index as usize)
                    .is_some_and(|k| *k == program.as_slice());
                if own && ix.data.len() >= 16 && ix.data[..8] == tag {
                    let d: [u8; 8] = ix.data[8..16].try_into()?;
                    let e = counts.entry(d).or_insert((0, ix.data[16..].to_vec()));
                    e.0 += 1;
                }
            }
        }
    }
    let trade: [u8; 8] = Sha256::digest(b"event:TradeEvent")[..8].try_into()?;
    let create: [u8; 8] = Sha256::digest(b"event:CreateEvent")[..8].try_into()?;
    if let Some((_, p)) = counts.get(&trade) {
        let u64_at = |o: usize| u64::from_le_bytes(p[o..o + 8].try_into().unwrap());
        println!(
            "TradeEvent example: mint {} sol {:.4} tokens {} is_buy {} user {} timestamp {}",
            bs58::encode(&p[0..32]).into_string(),
            u64_at(32) as f64 / 1e9,
            u64_at(40),
            p[48],
            bs58::encode(&p[49..81]).into_string(),
            i64::from_le_bytes(p[81..89].try_into().unwrap())
        );
    }
    if let Some((_, p)) = counts.get(&create) {
        let mut o = 0;
        let mut string = || {
            let len = u32::from_le_bytes(p[o..o + 4].try_into().unwrap()) as usize;
            let s = String::from_utf8_lossy(&p[o + 4..o + 4 + len]).into_owned();
            o += 4 + len;
            s
        };
        let (name, symbol, uri) = (string(), string(), string());
        println!(
            "CreateEvent example: name {name:?} symbol {symbol:?} uri {uri:?} mint {}",
            bs58::encode(&p[o..o + 32]).into_string()
        );
    }
    for (d, (n, example)) in &counts {
        let name = names.get(d).copied().unwrap_or("?");
        println!(
            "{n:>7}  {}  {name}  payload {} bytes",
            d.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            example.len()
        );
    }
    Ok(())
}
