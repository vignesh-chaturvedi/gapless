//! End-to-end supervisor scenarios against a scripted fake chain. Each one checks the property
//! Gapless exists for: every transaction from the covered slots is delivered exactly once.

mod fake;

use std::collections::HashSet;
use std::time::Duration;

use fake::{Ending, FakeChain, Script, TXS_PER_SLOT, UPDATES_PER_SLOT, sig};
use futures::StreamExt;
use gapless::{
    Builder, Control, Disconnect, DisconnectReason, Event, Gapless, Incident, Origin, SlotRange,
    SlotStatus, State, Transaction,
};
use tonic::Status;

const TIP: u64 = 1_000;
/// Slots 1001..=1020 in full, then two transactions into slot 1021.
const MID_SLOT_1021: usize = 20 * UPDATES_PER_SLOT + 2;

#[derive(Default)]
struct Run {
    txs: Vec<Transaction>,
    disconnects: Vec<Disconnect>,
    recovered: Vec<Incident>,
    states: Vec<State>,
    stopped: Option<String>,
}

fn config(horizon: u64) -> gapless::Config {
    Builder::new("test-key")
        .program("Prog1111111111111111111111111111111111111")
        .account_api(None)
        .replay_horizon(horizon)
        .into_config()
        .unwrap()
}

/// Collect events until slot `until` completes live, or the supervisor stops. `on_tx` can steer
/// the run after each delivered transaction.
async fn run_until(
    chain: FakeChain,
    horizon: u64,
    until: u64,
    mut on_tx: impl FnMut(usize, &Control),
) -> Run {
    let (mut events, control) = Gapless::with_source(config(horizon), chain).start();
    let mut run = Run::default();
    let collect = async {
        while let Some(event) = events.next().await {
            match event {
                Event::Transaction(tx) => {
                    run.txs.push(*tx);
                    on_tx(run.txs.len(), &control);
                }
                Event::Disconnected(d) => run.disconnects.push(d),
                Event::Recovered(i) => run.recovered.push(i),
                Event::State(State::Stopped { reason }) => {
                    run.stopped = Some(reason);
                    break;
                }
                Event::State(s) => run.states.push(s),
                Event::Slot {
                    slot,
                    status: SlotStatus::SlotProcessed,
                    origin: Origin::Live,
                } if slot >= until => break,
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(3_600), collect)
        .await
        .expect("scenario timed out");
    control.stop();
    run
}

/// Every transaction in `slots` arrived exactly once, and nothing arrived twice anywhere.
fn assert_exactly_once(run: &Run, slots: impl IntoIterator<Item = u64>) {
    let mut seen = HashSet::new();
    for tx in &run.txs {
        assert!(
            seen.insert(tx.signature.0),
            "slot {} index {} delivered twice",
            tx.slot,
            tx.index
        );
    }
    for slot in slots {
        for i in 0..TXS_PER_SLOT {
            assert!(
                seen.contains(&sig(slot, i)),
                "slot {slot} index {i} never delivered"
            );
        }
    }
}

#[tokio::test(start_paused = true)]
async fn live_stream_is_delivered_once() {
    let chain = FakeChain::new(TIP, 3_000, vec![]);
    let run = run_until(chain.clone(), 3_000, 1_100, |_, _| {}).await;
    assert_exactly_once(&run, 1_001..=1_100);
    assert!(run.disconnects.is_empty());
    assert_eq!(chain.requested_from_slots(), vec![None]);
    assert!(run.states.contains(&State::Live));
}

#[tokio::test(start_paused = true)]
async fn mid_slot_kill_recovers_without_loss_or_duplicates() {
    let chain = FakeChain::new(
        TIP,
        3_000,
        vec![Script::cut(MID_SLOT_1021, Ending::Killed, 50)],
    );
    let run = run_until(chain.clone(), 3_000, 1_200, |_, _| {}).await;

    assert_exactly_once(&run, 1_001..=1_200);
    let d = &run.disconnects[0];
    assert_eq!(d.reason, DisconnectReason::Killed);
    assert_eq!(d.last_complete_slot, Some(1_020));
    assert_eq!(
        d.resume_from,
        Some(1_021),
        "resume from the slot cut mid-stream, not after it"
    );
    // The last request is the handoff patch, re-reading from the replay target.
    assert_eq!(
        chain.requested_from_slots(),
        vec![None, Some(1_021), Some(1_071)]
    );

    let incident = &run.recovered[0];
    assert_eq!(
        incident.gap,
        Some(SlotRange {
            first: 1_021,
            last: 1_071
        })
    );
    assert_eq!(
        incident.duplicates, 2,
        "the two transactions of slot 1021 seen before the kill"
    );
    assert_eq!(incident.replayed, 51 * TXS_PER_SLOT - 2);
    assert!(incident.recovered_at.is_some());
    assert!(
        run.txs
            .iter()
            .filter(|tx| (1_022..=1_071).contains(&tx.slot))
            .all(|tx| tx.origin
                == Origin::Replay {
                    incident: incident.id
                })
    );
}

#[tokio::test(start_paused = true)]
async fn deep_replay_steps_through_backpressure_closes() {
    // Each replay connection ends cleanly after ~100 slots, three slot-straddling times over.
    let step = 100 * UPDATES_PER_SLOT + 3;
    let scripts = vec![
        Script::cut(MID_SLOT_1021, Ending::Killed, 400),
        Script::cut(step, Ending::Eof, 0),
        Script::cut(step, Ending::Eof, 0),
        Script::cut(step, Ending::Eof, 0),
    ];
    let chain = FakeChain::new(TIP, 3_000, scripts);
    let run = run_until(chain.clone(), 3_000, 1_500, |_, _| {}).await;

    assert_exactly_once(&run, 1_001..=1_500);
    assert_eq!(
        chain.requested_from_slots(),
        vec![
            None,
            Some(1_021),
            Some(1_121),
            Some(1_221),
            Some(1_321),
            Some(1_421)
        ],
        "four replay steps, then the handoff patch from the final target"
    );
    assert_eq!(
        run.recovered.len(),
        1,
        "one incident, however many steps it takes"
    );
    let incident = &run.recovered[0];
    assert_eq!(incident.steps.len(), 4);
    assert!(incident.steps[..3].iter().all(|s| {
        s.ended
            .as_ref()
            .is_some_and(|(r, _)| *r == DisconnectReason::ServerClosed)
    }));
    assert!(incident.steps[3].ended.is_none());
    assert_eq!(incident.duplicates, 2 + 3 * 3);
    assert!(run.disconnects.iter().all(|d| d.incident == incident.id));
    assert_eq!(
        run.disconnects.iter().map(|d| d.step).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
}

#[tokio::test(start_paused = true)]
async fn duplicates_sent_by_the_server_are_dropped() {
    let chain = FakeChain::new(
        TIP,
        3_000,
        vec![Script {
            duplicate_every: Some(5),
            ..Script::default()
        }],
    );
    let run = run_until(chain.clone(), 3_000, 1_100, |_, _| {}).await;
    assert_exactly_once(&run, 1_001..=1_100);
    assert!(chain.duplicates_sent() >= 80);
}

#[tokio::test(start_paused = true)]
async fn outage_beyond_the_horizon_reports_the_lost_slots() {
    let chain = FakeChain::new(
        TIP,
        100,
        vec![Script::cut(MID_SLOT_1021, Ending::Killed, 300)],
    );
    let run = run_until(chain.clone(), 100, 1_400, |_, _| {}).await;

    assert_eq!(
        chain.requested_from_slots(),
        vec![None, Some(1_221), Some(1_321)]
    );
    let incident = &run.recovered[0];
    assert_eq!(
        incident.unrecoverable,
        Some(SlotRange {
            first: 1_021,
            last: 1_220
        })
    );
    assert_exactly_once(&run, (1_001..=1_020).chain(1_221..=1_400));
    assert!(!run.txs.iter().any(|tx| (1_022..=1_220).contains(&tx.slot)));
}

#[tokio::test(start_paused = true)]
async fn rejected_subscription_stops() {
    let status = Status::permission_denied(
        "unfiltered/firehose subscriptions are not allowed on non-PAYG streams; narrow your filters",
    );
    let chain = FakeChain::new(TIP, 3_000, vec![Script::reject(status)]);
    let run = run_until(chain, 3_000, 1_100, |_, _| {}).await;
    assert_eq!(run.disconnects[0].reason, DisconnectReason::Rejected);
    assert!(run.stopped.unwrap().contains("rejected"));
    assert!(run.txs.is_empty());
}

#[tokio::test(start_paused = true)]
async fn failed_first_connect_retries_live() {
    let status = Status::unavailable("all geyser backends unavailable, please retry in a moment");
    let chain = FakeChain::new(TIP, 3_000, vec![Script::reject(status)]);
    let run = run_until(chain.clone(), 3_000, 1_100, |_, _| {}).await;
    assert_eq!(
        chain.requested_from_slots(),
        vec![None, None],
        "nothing seen yet, so nothing to replay"
    );
    assert_eq!(
        run.recovered[0].reason,
        DisconnectReason::BackendUnavailable
    );
    assert_eq!(run.recovered[0].gap, None);
    assert_exactly_once(&run, 1_001..=1_100);
}

#[tokio::test(start_paused = true)]
async fn stalled_stream_is_replaced() {
    let chain = FakeChain::new(
        TIP,
        3_000,
        vec![Script::cut(MID_SLOT_1021, Ending::Hang, 20)],
    );
    let run = run_until(chain.clone(), 3_000, 1_150, |_, _| {}).await;
    assert_eq!(run.disconnects[0].reason, DisconnectReason::Stalled);
    assert_eq!(
        chain.requested_from_slots(),
        vec![None, Some(1_021), Some(1_041)]
    );
    assert_exactly_once(&run, 1_001..=1_150);
}

#[tokio::test(start_paused = true)]
async fn client_cut_recovers() {
    let chain = FakeChain::new(TIP, 3_000, vec![]);
    let run = run_until(chain.clone(), 3_000, 1_200, |n, control| {
        if n == 150 {
            control.cut();
        }
    })
    .await;
    assert_eq!(run.disconnects[0].reason, DisconnectReason::Cut);
    assert_eq!(run.recovered.len(), 1);
    assert_exactly_once(&run, 1_001..=1_200);
}

#[tokio::test(start_paused = true)]
async fn hold_delays_the_outage_but_not_replay_steps() {
    let scripts = vec![
        Script::cut(MID_SLOT_1021, Ending::Killed, 300),
        Script::cut(100 * UPDATES_PER_SLOT, Ending::Eof, 0),
    ];
    let chain = FakeChain::new(TIP, 3_000, scripts);
    let (mut events, control) = Gapless::with_source(config(3_000), chain).start();
    control.hold(Some(Duration::from_secs(60)));
    let mut delays = Vec::new();
    while let Some(event) = events.next().await {
        match event {
            Event::State(State::Backoff { delay, .. }) => delays.push(delay),
            Event::Recovered(_) => break,
            _ => {}
        }
    }
    control.stop();
    assert_eq!(delays.len(), 2);
    assert_eq!(
        delays[0],
        Duration::from_secs(60),
        "the outage itself is held"
    );
    assert!(
        delays[1] < Duration::from_secs(1),
        "the replay step retries at once: {:?}",
        delays[1]
    );
}

fn handoff_scripts() -> Vec<Script> {
    vec![
        Script::cut(MID_SLOT_1021, Ending::Killed, 50),
        // The resumed stream loses the first three transactions of slot 1072, the first slot
        // past the tip when it switched to live.
        Script {
            handoff_loss: 3,
            ..Script::default()
        },
    ]
}

#[tokio::test(start_paused = true)]
async fn handoff_patch_recovers_what_the_live_switch_dropped() {
    let chain = FakeChain::new(TIP, 3_000, handoff_scripts());
    let (mut events, control) = Gapless::with_source(config(3_000), chain.clone()).start();
    let mut run = Run::default();
    let mut patched = None;
    while let Some(event) = events.next().await {
        match event {
            Event::Transaction(tx) => run.txs.push(*tx),
            Event::HandoffPatched {
                slots,
                recovered,
                error,
                ..
            } => {
                patched = Some((slots, recovered, error));
            }
            Event::Slot {
                slot,
                status: SlotStatus::SlotProcessed,
                origin: Origin::Live,
            } if slot >= 1_200 && patched.is_some() => break,
            _ => {}
        }
    }
    control.stop();
    let (slots, recovered, error) = patched.expect("the patch ran");
    assert_eq!(
        slots,
        SlotRange {
            first: 1_071,
            last: 1_091
        }
    );
    assert_eq!(recovered, 3);
    assert_eq!(error, None);
    assert_eq!(
        chain.requested_from_slots(),
        vec![None, Some(1_021), Some(1_071)]
    );
    assert_exactly_once(&run, 1_001..=1_200);
}

#[tokio::test(start_paused = true)]
async fn without_the_patch_the_handoff_loss_stays_missing() {
    let chain = FakeChain::new(TIP, 3_000, handoff_scripts());
    let config = Builder::new("test-key")
        .program("Prog1111111111111111111111111111111111111")
        .account_api(None)
        .handoff_patch(false)
        .into_config()
        .unwrap();
    let (mut events, control) = Gapless::with_source(config, chain).start();
    let mut delivered = HashSet::new();
    while let Some(event) = events.next().await {
        match event {
            Event::Transaction(tx) => {
                delivered.insert(tx.signature.0);
            }
            Event::Slot {
                slot,
                status: SlotStatus::SlotProcessed,
                origin: Origin::Live,
            } if slot >= 1_200 => break,
            _ => {}
        }
    }
    control.stop();
    let lost: Vec<u64> = (0..TXS_PER_SLOT)
        .filter(|i| !delivered.contains(&sig(1_072, *i)))
        .collect();
    assert_eq!(
        lost,
        vec![0, 1, 2],
        "the first three transactions of the handoff slot"
    );
}
