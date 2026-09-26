//! Exactly-once, gap-aware consumption of Solami's Yellowstone gRPC stream.
//!
//! Gapless wraps a transaction subscription and keeps it whole across disconnects. It tracks
//! which slots have fully arrived and resumes with `from_slot` after a drop, in several steps
//! if Solami closes a deep replay for backpressure. It drops the duplicates a resume produces
//! and reports every outage as an [`Incident`].
//!
//! ```no_run
//! # async fn run() -> Result<(), gapless::Error> {
//! use futures::StreamExt;
//!
//! let (mut events, _control) = gapless::Gapless::builder("<solami api key>")
//!     .program("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
//!     .build()?
//!     .start();
//! while let Some(event) = events.next().await {
//!     if let gapless::Event::Transaction(tx) = event {
//!         println!("{} in slot {}", tx.signature, tx.slot);
//!     }
//! }
//! # Ok(()) }
//! ```

mod account;
mod backoff;
mod classify;
mod config;
mod cursor;
mod dedup;
mod event;
pub mod fixture;
mod source;
mod supervisor;

pub use account::{AccountApi, ClosedConnection, LiveConnection};
pub use backoff::Backoff;
pub use classify::DisconnectReason;
pub use config::{
    BackoffConfig, Builder, Config, DEFAULT_API_URL, DEFAULT_GRPC_URL, REPLAY_HORIZON,
};
pub use cursor::SlotCursor;
pub use dedup::DedupWindow;
pub use event::{
    Disconnect, Event, Incident, Metrics, Origin, ReplayStep, Signature, SlotRange, State,
    Transaction,
};
pub use source::{SolamiSource, Source, UpdateStream};
pub use supervisor::{Control, Events, Gapless};

/// The Solami SDK, re-exported so consumers can read transaction internals without a direct dependency.
pub use solami;
pub use solami::geyser::{CommitmentLevel, SlotStatus};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(String),
    #[error("gRPC transport: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("account API: {0}")]
    Account(String),
}
