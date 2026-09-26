//! Run Gapless live (optionally breaking it on purpose), then check everything it delivered
//! against Solami's RPC.
//!
//! ```bash
//! # stream 2 minutes, kill the stream after 20 s and stay offline 60 s, then verify
//! cargo run -p gapless-verify --release -- run --secs 120 --kill-after 20 --hold 60
//!
//! # check the expected-set source itself against full blocks
//! cargo run -p gapless-verify --release -- sources --slots 10
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use gapless::{AccountApi, DEFAULT_API_URL, Event, Gapless, Incident, SlotRange, SlotStatus};
use gapless_verify::{DEFAULT_RPC_URL, Report, Verifier};
use serde_json::json;

const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
/// Slots after a replay target where Solami's switch to the live feed can drop transactions.
/// Phase 2 saw it 7-8 slots in: the slot executing when the live feed attached.
const HANDOFF_SLOTS: u64 = 64;

#[derive(Parser)]
#[command(
    name = "gapless-verify",
    about = "Verify a live Gapless stream against Solami's RPC"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Stream live, optionally kill the stream partway, then verify the whole window and each incident.
    Run {
        /// How long to stream (extended until an open incident recovers).
        #[arg(long, default_value_t = 90)]
        secs: u64,
        /// Kill our stream through Solami's account API after this many seconds.
        #[arg(long)]
        kill_after: Option<u64>,
        /// Stay offline this long after the kill, so there's a real gap to replay.
        #[arg(long, default_value_t = 0)]
        hold: u64,
        /// Verify the same window this many times and require identical reports.
        #[arg(long, default_value_t = 2)]
        repeat: usize,
        /// Write the reports as JSON here.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Compare getTransactionsForAddress with full blocks, slot by slot.
    Sources {
        /// Number of recent finalized slots to check.
        #[arg(long, default_value_t = 10)]
        slots: u64,
    },
}

struct Env {
    api_key: String,
    rpc_url: String,
    program: String,
}

impl Env {
    fn load() -> Result<Self> {
        dotenvy::dotenv().ok();
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        Ok(Self {
            api_key: var("SOLAMI_API_KEY")
                .context("SOLAMI_API_KEY is not set (see .env.example)")?,
            rpc_url: var("SOLAMI_RPC_URL").unwrap_or_else(|| DEFAULT_RPC_URL.to_owned()),
            program: var("GAPLESS_PROGRAM").unwrap_or_else(|| PUMP_FUN.to_owned()),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let env = Env::load()?;
    match cli.cmd {
        Cmd::Run {
            secs,
            kill_after,
            hold,
            repeat,
            out,
        } => run(&env, secs, kill_after, hold, repeat, out).await,
        Cmd::Sources { slots } => sources(&env, slots).await,
    }
}

/// What the stream delivered during a run.
#[derive(Default)]
struct Session {
    delivered: HashMap<gapless::Signature, u64>,
    repeats: u64,
    first_complete: Option<u64>,
    last_complete: Option<u64>,
    incidents: Vec<Incident>,
}

async fn run(
    env: &Env,
    secs: u64,
    kill_after: Option<u64>,
    hold: u64,
    repeat: usize,
    out: Option<PathBuf>,
) -> Result<()> {
    let config = Gapless::builder(&env.api_key)
        .program(&env.program)
        .into_config()?;
    let verifier = Verifier::for_stream(&config, &env.rpc_url)?;
    let account = AccountApi::new(DEFAULT_API_URL, &env.api_key);
    let (mut events, control) = Gapless::new(config)?.start();
    if hold > 0 {
        control.hold(Some(Duration::from_secs(hold)));
    }

    let started = Instant::now();
    let at = || format!("{:>6.1}s", started.elapsed().as_secs_f64());
    let mut session = Session::default();
    let mut conn_id: Option<String> = None;
    let mut killed = false;
    let mut open_incident = false;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    println!("{} streaming {} for {secs}s", at(), env.program);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let elapsed = started.elapsed().as_secs();
                if let (Some(after), false, Some(id)) = (kill_after, killed, conn_id.as_deref())
                    && elapsed >= after
                {
                    killed = true;
                    let ok = account.kill(id).await.context("kill through the account API")?;
                    println!("{} killed connection {id} through Solami's account API ({ok})", at());
                }
                // Keep going past the deadline until an open incident recovers (up to 5 minutes more).
                if elapsed >= secs && (!open_incident || elapsed >= secs + 300) {
                    break;
                }
            }
            event = events.next() => match event {
                None => bail!("the stream ended unexpectedly"),
                Some(Event::Transaction(tx)) => {
                    if session.delivered.insert(tx.signature, tx.slot).is_some() {
                        session.repeats += 1;
                    }
                }
                Some(Event::Slot { slot, status: SlotStatus::SlotProcessed, .. }) => {
                    session.first_complete.get_or_insert(slot);
                    session.last_complete = Some(session.last_complete.map_or(slot, |s| s.max(slot)));
                }
                Some(Event::ConnectionIdentified { conn_id: id }) => conn_id = Some(id),
                Some(Event::Disconnected(d)) => {
                    open_incident = true;
                    println!("{} disconnected (incident #{} step {}): {}", at(), d.incident, d.step, d.reason);
                }
                Some(Event::ReasonResolved { incident, step, reason }) => {
                    println!("{} incident #{incident} step {step}: Solami's history says {reason}", at());
                }
                Some(Event::Recovered(incident)) => {
                    open_incident = false;
                    println!(
                        "{} recovered incident #{}: gap {:?}, {} step(s), {} replayed, {} duplicates dropped",
                        at(), incident.id, incident.gap, incident.steps.len(), incident.replayed, incident.duplicates
                    );
                    session.incidents.push(incident);
                }
                Some(Event::HandoffPatched { incident, slots, recovered, error }) => {
                    println!(
                        "{} incident #{incident}: handoff patch re-read slots {}..={}, recovered {recovered}{}",
                        at(), slots.first, slots.last,
                        error.map(|e| format!(" (error: {e})")).unwrap_or_default()
                    );
                }
                Some(Event::State(gapless::State::Stopped { reason })) => bail!("stream stopped: {reason}"),
                Some(_) => {}
            }
        }
    }
    control.stop();

    let (Some(first), Some(last)) = (session.first_complete, session.last_complete) else {
        bail!("no complete slots were seen");
    };
    // The first slots may have started before we connected; begin two slots in.
    let window = SlotRange::new(first + 2, last);
    println!(
        "{} delivered {} transactions ({} repeats seen by this consumer); waiting for slot {} to finalize",
        at(),
        session.delivered.len(),
        session.repeats,
        window.last
    );
    verifier
        .wait_until_finalized(window.last, Duration::from_secs(180))
        .await?;

    let mut reports: Vec<Report> = Vec::new();
    for n in 1..=repeat.max(1) {
        let report = verifier.verify(window, &session.delivered).await?;
        println!("\nwhole window, pass {n}: {}", report.summary());
        reports.push(report);
    }
    let fingerprints: Vec<String> = reports.iter().map(Report::fingerprint).collect();
    let stable = fingerprints.windows(2).all(|w| w[0] == w[1]);
    println!(
        "{} passes: {}",
        reports.len(),
        if stable {
            "identical reports"
        } else {
            "REPORTS DIFFER"
        }
    );

    // Explain and repair what the window is missing.
    let handoffs: Vec<(u64, SlotRange)> = session
        .incidents
        .iter()
        .flat_map(|i| {
            i.steps.iter().map(move |s| {
                (
                    i.id,
                    SlotRange::new(s.target_slot, s.target_slot + HANDOFF_SLOTS),
                )
            })
        })
        .collect();
    let mut window_report = reports.pop().expect("at least one pass");
    window_report.explain_handoffs(&handoffs);
    verifier.repair(&mut window_report).await?;
    println!("\nwhole window, explained: {}", window_report.summary());

    let mut incident_reports = Vec::new();
    for incident in &session.incidents {
        let Some(gap) = incident.gap else { continue };
        // Cover the replayed gap and the slots after it where the live feed takes over.
        let target = incident.steps.last().map_or(gap.last, |s| s.target_slot);
        let range = SlotRange::new(
            gap.first.max(window.first),
            (target + HANDOFF_SLOTS).min(window.last),
        );
        if range.first > range.last {
            continue;
        }
        let mut report = verifier.verify(range, &session.delivered).await?;
        report.explain_handoffs(&handoffs);
        verifier.repair(&mut report).await?;
        println!(
            "\nincident #{} (gap {}..={} plus {} handoff slots): {}",
            incident.id,
            gap.first,
            gap.last,
            range.last.saturating_sub(gap.last),
            report.summary()
        );
        incident_reports.push(json!({
            "incident": incident.id,
            "reason": incident.reason.to_string(),
            "steps": incident.steps.len(),
            "replayed": incident.replayed,
            "duplicates_dropped": incident.duplicates,
            "report": report,
        }));
    }

    if let Some(path) = out {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let doc = json!({
            "program": env.program,
            "delivered": session.delivered.len(),
            "consumer_repeats": session.repeats,
            "stable_across_passes": stable,
            "window": window_report,
            "incidents": incident_reports,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
        println!("\nwrote {}", path.display());
    }
    if !stable {
        bail!("verification passes disagreed");
    }
    Ok(())
}

async fn sources(env: &Env, slots: u64) -> Result<()> {
    let verifier = Verifier::new(&env.rpc_url, &env.api_key, vec![env.program.clone()], false);
    let tip = verifier.finalized_tip().await?;
    let range = SlotRange::new(tip - slots + 1, tip);
    let started = Instant::now();
    let checks = verifier.cross_check(range).await?;
    let mut total_block = 0;
    let mut total_history = 0;
    let mut lookups = 0;
    for c in &checks {
        total_block += c.from_block;
        total_history += c.from_history;
        lookups += c.via_lookup_table;
        println!(
            "slot {}  block {:>3}  history {:>3}  via lookup tables {:>2}  {}",
            c.slot,
            c.from_block,
            c.from_history,
            c.via_lookup_table,
            if !c.has_block {
                "no block"
            } else if c.agree {
                "agree"
            } else {
                "DISAGREE"
            }
        );
    }
    let agree = checks.iter().filter(|c| c.agree).count();
    println!(
        "\n{agree}/{} slots agree; {total_block} from blocks vs {total_history} from getTransactionsForAddress ({lookups} via lookup tables); {:.1}s",
        checks.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
