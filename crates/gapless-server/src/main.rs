//! gapless-server: runs Gapless against Solami (or a recorded fixture), and serves the console
//! a live feed, chaos controls and verified incidents.
//!
//! ```bash
//! cargo run -p gapless-server --release                                   # live, needs SOLAMI_API_KEY
//! cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst
//! ```

mod api;
mod app;
mod dto;
mod engine;
mod indexer;
mod ledger;
mod store;
mod truth;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use clap::Parser;
use gapless::fixture::{Fixture, FixtureSource};
use gapless::{AccountApi, DEFAULT_API_URL, Gapless};
use gapless_verify::DEFAULT_RPC_URL;
use tokio::sync::broadcast;

use crate::app::{App, Chaos, Shared};
use crate::dto::{ControlsDto, MetricsDto, Snapshot, StateDto, now_ms};
use crate::store::Store;
use crate::truth::{FixtureTruth, RpcTruth, Truth};

const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

#[derive(Parser)]
#[command(
    name = "gapless-server",
    about = "Serve a live, verified Gapless stream to the console"
)]
struct Cli {
    /// Play a recorded fixture instead of connecting to Solami (no API key needed).
    #[arg(long)]
    offline: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:8790")]
    listen: SocketAddr,
    /// SQLite file for incident history.
    #[arg(long, default_value = "gapless.db")]
    db: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gapless_server=info,gapless=info".into()),
        )
        .init();
    let cli = Cli::parse();
    let var = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let program = var("GAPLESS_PROGRAM").unwrap_or_else(|| PUMP_FUN.to_owned());

    let (gapless, truth, account, fixture, mode): (
        Gapless,
        Arc<dyn Truth>,
        Option<AccountApi>,
        Option<FixtureSource>,
        &'static str,
    ) = match &cli.offline {
        Some(path) => {
            let fixture =
                Fixture::load(path).with_context(|| format!("loading {}", path.display()))?;
            tracing::info!(
                "offline: {} frames, slots {:?}, {:.0}s per loop",
                fixture.frames().len(),
                fixture.slots(),
                fixture.duration().as_secs_f64()
            );
            let source = FixtureSource::new(fixture);
            let config = Gapless::builder("offline")
                .program(&program)
                .account_api(None)
                .into_config()?;
            let truth = FixtureTruth::new(source.clone(), program.clone());
            (
                Gapless::with_source(config, source.clone()),
                truth,
                None,
                Some(source),
                "offline",
            )
        }
        None => {
            let key = var("SOLAMI_API_KEY")
                .context("SOLAMI_API_KEY is not set (or pass --offline <fixture>)")?;
            let rpc_url = var("SOLAMI_RPC_URL").unwrap_or_else(|| DEFAULT_RPC_URL.to_owned());
            let api_url = var("SOLAMI_API_URL").unwrap_or_else(|| DEFAULT_API_URL.to_owned());
            let mut builder = Gapless::builder(&key)
                .program(&program)
                .account_api(Some(api_url.clone()));
            if let Some(url) = var("SOLAMI_GRPC_URL") {
                builder = builder.grpc_url(url);
            }
            let config = builder.into_config()?;
            let truth: Arc<dyn Truth> = Arc::new(RpcTruth::new(&config, &rpc_url)?);
            (
                Gapless::new(config)?,
                truth,
                Some(AccountApi::new(api_url, &key)),
                None,
                "live",
            )
        }
    };

    let store = Store::open(&cli.db)?;
    let session = store.start_session(now_ms(), mode, &program).await?;
    let (events, control) = gapless.start();
    let (hub, _) = broadcast::channel(1_024);
    let snapshot = Snapshot {
        mode,
        program: program.clone(),
        started_at: now_ms(),
        state: StateDto::Connecting { attempt: 1 },
        state_since: now_ms(),
        conn_id: None,
        metrics: MetricsDto::default(),
        tip: None,
        finalized: None,
        solami: None,
        verified_through: None,
        controls: ControlsDto {
            handoff_patch: true,
            throttle_ms: None,
            hold_secs: None,
            can_kill: false,
        },
        indexer: Default::default(),
        open_incident: None,
    };
    let incidents = store
        .list(20)
        .await?
        .into_iter()
        .filter_map(|v| serde_json::from_value::<serde_json::Value>(v).ok())
        .collect::<Vec<_>>();
    let app = Arc::new(App {
        mode,
        program,
        control,
        account,
        fixture,
        store,
        hub,
        shared: RwLock::new(Shared {
            snapshot,
            tape: Vec::new(),
            incidents: Vec::new(),
            txs: VecDeque::new(),
            log: VecDeque::new(),
        }),
        chaos: Mutex::new(Chaos {
            handoff_patch: true,
            throttle_ms: None,
            hold_secs: None,
            pending_label: None,
        }),
    });
    tracing::info!(
        "{} earlier incident(s) in {}",
        incidents.len(),
        cli.db.display()
    );

    tokio::spawn(engine::run(app.clone(), truth, events, session));
    let listener = tokio::net::TcpListener::bind(cli.listen).await?;
    tracing::info!("listening on http://{}", cli.listen);
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}
