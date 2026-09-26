//! Phase 0 spike tool. Each subcommand answers one question about how Solami behaves with a
//! real key; the answers are written up in docs/spike.md.

mod api;
mod blocks;
mod env;
mod inspect;
mod rpc;
mod stream;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use solami::geyser::CommitmentLevel;

#[derive(Parser)]
#[command(
    name = "gapless-probe",
    about = "Check the Solami primitives Gapless depends on"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Spike 1: stream a program's transactions; report rate, latency and slot statuses.
    Stream {
        #[arg(long, default_value_t = 30)]
        secs: u64,
        #[arg(long, value_enum, default_value_t = Commitment::Processed)]
        commitment: Commitment,
        /// Don't answer server pings.
        #[arg(long)]
        no_pong: bool,
    },
    /// Spike 2: read the replay horizon, then subscribe with from_slot = tip - back.
    Replay {
        #[arg(long, default_value_t = 500)]
        back: u64,
        #[arg(long, default_value_t = 90)]
        secs: u64,
        /// Ask the server for zstd-compressed responses.
        #[arg(long)]
        zstd: bool,
    },
    /// Spike 3: call the account API's connection routes with each auth style.
    Connections,
    /// Spike 4: open a stream, kill it through the account API, then resume from its last slot.
    Kill {
        #[arg(long, default_value_t = 10)]
        after: u64,
    },
    /// Spike 5: fetch a window of consecutive confirmed blocks and time it.
    Blocks {
        #[arg(long, default_value_t = 150)]
        count: u64,
        #[arg(long, default_value_t = 16)]
        concurrency: usize,
    },
    /// Try Solami's unary Geyser RPCs and print subscribe response metadata.
    Unary,
    /// Summarize a capture written by `record`.
    Inspect { path: PathBuf },
    /// Count the target program's instructions in a capture, by Anchor discriminator.
    Analyze { path: PathBuf },
    /// Count the target program's Anchor events in a capture.
    Events { path: PathBuf },
    /// Cut a capture down to a committed fixture: a time window, logs and balances stripped,
    /// zstd-compressed.
    Trim {
        path: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Seconds to skip from the start.
        #[arg(long, default_value_t = 10)]
        skip: u64,
        #[arg(long, default_value_t = 150)]
        secs: u64,
    },
    /// Record the live stream to fixtures/raw/ for offline development.
    Record {
        #[arg(long, default_value_t = 600)]
        secs: u64,
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Commitment {
    Processed,
    Confirmed,
    Finalized,
}

impl From<Commitment> for CommitmentLevel {
    fn from(c: Commitment) -> Self {
        match c {
            Commitment::Processed => CommitmentLevel::Processed,
            Commitment::Confirmed => CommitmentLevel::Confirmed,
            Commitment::Finalized => CommitmentLevel::Finalized,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Inspect { path } => return inspect::inspect(path),
        Cmd::Analyze { path } => return inspect::analyze(path),
        Cmd::Events { path } => return inspect::events(path),
        Cmd::Trim {
            path,
            out,
            skip,
            secs,
        } => return inspect::trim(path, out, *skip, *secs),
        _ => {}
    }
    let env = env::Env::load()?;
    match cli.cmd {
        Cmd::Stream {
            secs,
            commitment,
            no_pong,
        } => stream::stream(&env, secs, commitment.into(), !no_pong).await,
        Cmd::Replay { back, secs, zstd } => stream::replay(&env, back, secs, zstd).await,
        Cmd::Connections => api::connections(&env).await,
        Cmd::Kill { after } => stream::kill(&env, after).await,
        Cmd::Blocks { count, concurrency } => blocks::blocks(&env, count, concurrency).await,
        Cmd::Record { secs, out } => stream::record(&env, secs, out).await,
        Cmd::Unary => stream::unary(&env).await,
        Cmd::Inspect { .. } | Cmd::Analyze { .. } | Cmd::Events { .. } | Cmd::Trim { .. } => {
            unreachable!("handled before loading the environment")
        }
    }
}
