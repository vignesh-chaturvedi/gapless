//! Live tail of a program's transactions through Gapless, printing every state change,
//! disconnect and recovery. Kill the stream from Solami's dashboard (or the account API) while
//! it runs and watch it come back without losing or repeating a transaction.
//!
//! ```bash
//! cargo run -p gapless --example tail --release
//! ```
//!
//! Reads `SOLAMI_API_KEY`, and optionally `GAPLESS_PROGRAM` and `GAPLESS_HOLD_SECS`, from the
//! environment or `.env`.

use std::collections::HashSet;
use std::time::Instant;

use futures::StreamExt;
use gapless::{Event, Gapless, Origin, State};

const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let key = std::env::var("SOLAMI_API_KEY")?;
    let program = std::env::var("GAPLESS_PROGRAM").unwrap_or_else(|_| PUMP_FUN.to_owned());

    let (mut events, control) = Gapless::builder(key).program(&program).build()?.start();
    // GAPLESS_HOLD_SECS=60 keeps the client offline for a minute after each drop, turning a
    // kill into a real outage that has to be replayed.
    if let Some(secs) = std::env::var("GAPLESS_HOLD_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        control.hold(Some(std::time::Duration::from_secs(secs)));
    }
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        control.stop();
    });

    let start = Instant::now();
    let at = || format!("{:>7.1}s", start.elapsed().as_secs_f64());
    // The consumer's own check: a signature showing up twice here would be a Gapless bug.
    let mut seen = HashSet::new();
    let (mut live, mut replayed, mut repeats) = (0u64, 0u64, 0u64);

    println!("{} following {program}", at());
    while let Some(event) = events.next().await {
        match event {
            Event::Transaction(tx) => {
                if !seen.insert(tx.signature) {
                    repeats += 1;
                }
                match tx.origin {
                    Origin::Live => live += 1,
                    Origin::Replay { .. } => replayed += 1,
                }
            }
            Event::State(state) => match state {
                State::Connecting { attempt } => {
                    println!("{} connecting (attempt {attempt})", at())
                }
                State::Replaying {
                    incident,
                    from_slot,
                    target_slot,
                } => println!(
                    "{} replaying incident #{incident}: from_slot {from_slot} → tip {target_slot} ({} slots)",
                    at(),
                    target_slot.saturating_sub(from_slot) + 1
                ),
                State::Live => println!("{} live", at()),
                State::Backoff { delay, .. } => {
                    println!("{} reconnecting in {} ms", at(), delay.as_millis())
                }
                State::Stopped { reason } => println!("{} stopped: {reason}", at()),
            },
            Event::ConnectionIdentified { conn_id } => {
                println!("{} Solami connection {conn_id}", at())
            }
            Event::Disconnected(d) => println!(
                "{} DISCONNECTED incident #{} step {}: {} ({}); last complete slot {:?}, resume from {:?}",
                at(),
                d.incident,
                d.step,
                d.reason,
                d.detail,
                d.last_complete_slot,
                d.resume_from
            ),
            Event::ReasonResolved {
                incident,
                step,
                reason,
            } => println!(
                "{} incident #{incident} step {step}: Solami's history says {reason}",
                at()
            ),
            Event::Recovered(i) => {
                let gap = i
                    .gap
                    .map(|g| format!("{}..={} ({} slots)", g.first, g.last, g.len()));
                println!(
                    "{} RECOVERED incident #{} after {:.1}s: gap {}, {} step(s), {} replayed, {} duplicates dropped{}",
                    at(),
                    i.id,
                    i.duration().unwrap_or_default().as_secs_f64(),
                    gap.as_deref().unwrap_or("none"),
                    i.steps.len(),
                    i.replayed,
                    i.duplicates,
                    i.unrecoverable
                        .map(|u| format!(", UNRECOVERABLE {}..={}", u.first, u.last))
                        .unwrap_or_default()
                );
            }
            Event::HandoffPatched {
                incident,
                slots,
                recovered,
                error,
            } => println!(
                "{} incident #{incident}: handoff patch re-read slots {}..={} and recovered {recovered} dropped transaction(s){}",
                at(),
                slots.first,
                slots.last,
                error
                    .map(|e| format!(" (patch error: {e})"))
                    .unwrap_or_default()
            ),
            Event::Metrics(m) => println!(
                "{} {:>4.0} tx/s  slot {}  lag {}  latency {}  delivered {}  dropped dupes {}  consumer repeats {repeats}",
                at(),
                m.transactions_per_sec,
                m.highest_complete.map_or("-".into(), |s| s.to_string()),
                m.lag_slots.map_or("-".into(), |l| l.to_string()),
                m.latency_ms.map_or("-".into(), |l| format!("{l:.0}ms")),
                m.delivered,
                m.duplicates,
            ),
            Event::Slot { .. } => {}
        }
    }
    println!(
        "done: {live} live + {replayed} replayed transactions, {repeats} repeats seen by the consumer"
    );
    Ok(())
}
