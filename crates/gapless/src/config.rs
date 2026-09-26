use std::time::Duration;

use solami::geyser::CommitmentLevel;

use crate::Error;
use crate::supervisor::Gapless;

pub const DEFAULT_GRPC_URL: &str = "https://grpc.solami.dev";
pub const DEFAULT_API_URL: &str = "https://api.solami.dev";

/// How far back Solami can replay with `from_slot`. `SubscribeReplayInfo` reported ~3,000 slots
/// in Phase 0, below the 3,500 in the docs.
pub const REPLAY_HORIZON: u64 = 3_000;

#[derive(Clone, Debug)]
pub struct Config {
    pub grpc_url: String,
    pub api_key: String,
    /// Transactions touching any of these accounts are delivered.
    pub account_include: Vec<String>,
    pub account_required: Vec<String>,
    pub account_exclude: Vec<String>,
    pub include_failed: bool,
    pub commitment: CommitmentLevel,
    /// Ask Solami for zstd-compressed responses. Less on the wire means a replay gets further
    /// before the server's buffer fills.
    pub compression: bool,
    /// Base URL of Solami's account API, used to identify our connection and to learn why a
    /// stream ended. `None` turns that off.
    pub account_api: Option<String>,
    pub replay_horizon: u64,
    /// Drop and reconnect when nothing (not even a ping) arrives for this long.
    pub stall_timeout: Duration,
    /// How often to ask for the chain tip while streaming.
    pub tip_interval: Duration,
    /// Events buffered for the consumer before the supervisor waits on it.
    pub event_buffer: usize,
    pub backoff: BackoffConfig,
}

impl Config {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            grpc_url: DEFAULT_GRPC_URL.to_owned(),
            api_key: api_key.into(),
            account_include: Vec::new(),
            account_required: Vec::new(),
            account_exclude: Vec::new(),
            include_failed: false,
            commitment: CommitmentLevel::Processed,
            compression: true,
            account_api: Some(DEFAULT_API_URL.to_owned()),
            replay_horizon: REPLAY_HORIZON,
            stall_timeout: Duration::from_secs(30),
            tip_interval: Duration::from_secs(2),
            event_buffer: 16_384,
            backoff: BackoffConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.api_key.trim().is_empty() {
            return Err(Error::Config("the Solami API key is empty".into()));
        }
        if self.account_include.is_empty() && self.account_required.is_empty() {
            // Plan-included streams reject unscoped (firehose) transaction filters.
            return Err(Error::Config(
                "add at least one program or account to follow".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct BackoffConfig {
    pub initial: Duration,
    pub max: Duration,
    /// Reconnects allowed per `window`. Solami locks an IP out past 100 per 10 s.
    pub max_per_window: usize,
    pub window: Duration,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial: Duration::from_millis(100),
            max: Duration::from_secs(10),
            max_per_window: 20,
            window: Duration::from_secs(10),
        }
    }
}

pub struct Builder {
    config: Config,
}

impl Builder {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            config: Config::new(api_key),
        }
    }

    /// Follow transactions that touch this program (or any account).
    pub fn program(mut self, address: impl Into<String>) -> Self {
        self.config.account_include.push(address.into());
        self
    }

    pub fn account_include<I, S>(mut self, addresses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config
            .account_include
            .extend(addresses.into_iter().map(Into::into));
        self
    }

    pub fn account_required<I, S>(mut self, addresses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config
            .account_required
            .extend(addresses.into_iter().map(Into::into));
        self
    }

    pub fn account_exclude<I, S>(mut self, addresses: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config
            .account_exclude
            .extend(addresses.into_iter().map(Into::into));
        self
    }

    pub fn include_failed(mut self, include: bool) -> Self {
        self.config.include_failed = include;
        self
    }

    pub fn commitment(mut self, commitment: CommitmentLevel) -> Self {
        self.config.commitment = commitment;
        self
    }

    pub fn grpc_url(mut self, url: impl Into<String>) -> Self {
        self.config.grpc_url = url.into();
        self
    }

    /// Point at a different account API, or pass `None` to run without it.
    pub fn account_api(mut self, base_url: Option<String>) -> Self {
        self.config.account_api = base_url;
        self
    }

    pub fn compression(mut self, on: bool) -> Self {
        self.config.compression = on;
        self
    }

    pub fn replay_horizon(mut self, slots: u64) -> Self {
        self.config.replay_horizon = slots;
        self
    }

    pub fn stall_timeout(mut self, timeout: Duration) -> Self {
        self.config.stall_timeout = timeout;
        self
    }

    pub fn backoff(mut self, backoff: BackoffConfig) -> Self {
        self.config.backoff = backoff;
        self
    }

    /// The validated configuration, for use with [`Gapless::with_source`].
    pub fn into_config(self) -> Result<Config, Error> {
        self.config.validate()?;
        Ok(self.config)
    }

    pub fn build(self) -> Result<Gapless, Error> {
        Gapless::new(self.into_config()?)
    }
}
