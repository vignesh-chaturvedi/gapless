//! Where verification gets its ground truth: Solami's RPC when live, the fixture offline.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use futures::future::BoxFuture;
use gapless::fixture::FixtureSource;
use gapless::{Signature, SlotRange};
use gapless_verify::{GroundTruth, Landed, Options, Report, RpcStats, Verifier, build_report};

/// Finalization lag the fixture pretends to have.
const FIXTURE_FINALITY: u64 = 32;

pub trait Truth: Send + Sync + 'static {
    fn finalized_tip(&self) -> BoxFuture<'_, Result<u64>>;
    /// The processed tip, for the console's timeline.
    fn tip(&self) -> BoxFuture<'_, Result<u64>>;
    /// `thorough` adds `getBlock` spot checks (live), which cost ~5 MB each.
    fn verify<'a>(
        &'a self,
        range: SlotRange,
        delivered: &'a HashMap<Signature, u64>,
        thorough: bool,
    ) -> BoxFuture<'a, Result<Report>>;
    fn repair<'a>(&'a self, report: &'a mut Report) -> BoxFuture<'a, Result<()>>;
}

/// Solami RPC: `getTransactionsForAddress`, plus `getBlock` spot checks when thorough.
pub struct RpcTruth {
    thorough: Verifier,
    light: Verifier,
}

impl RpcTruth {
    pub fn new(config: &gapless::Config, rpc_url: &str) -> Result<Self> {
        let thorough = Verifier::for_stream(config, rpc_url)?;
        let light = Verifier::for_stream(config, rpc_url)?.with_options(Options {
            spot_checks: 0,
            explain_checks: 2,
            ..Options::default()
        });
        Ok(Self { thorough, light })
    }
}

impl Truth for RpcTruth {
    fn finalized_tip(&self) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async { Ok(self.light.finalized_tip().await?) })
    }

    fn tip(&self) -> BoxFuture<'_, Result<u64>> {
        Box::pin(async { Ok(self.light.rpc().slot("processed").await?) })
    }

    fn verify<'a>(
        &'a self,
        range: SlotRange,
        delivered: &'a HashMap<Signature, u64>,
        thorough: bool,
    ) -> BoxFuture<'a, Result<Report>> {
        let verifier = if thorough {
            &self.thorough
        } else {
            &self.light
        };
        Box::pin(async move { Ok(verifier.verify(range, delivered).await?) })
    }

    fn repair<'a>(&'a self, report: &'a mut Report) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(self.light.repair(report).await?) })
    }
}

/// Offline: the fixture knows exactly what it played.
pub struct FixtureTruth {
    source: FixtureSource,
    program: String,
}

impl FixtureTruth {
    pub fn new(source: FixtureSource, program: String) -> Arc<Self> {
        Arc::new(Self { source, program })
    }
}

impl Truth for FixtureTruth {
    fn finalized_tip(&self) -> BoxFuture<'_, Result<u64>> {
        let tip = self.source.current_tip().saturating_sub(FIXTURE_FINALITY);
        Box::pin(async move { Ok(tip) })
    }

    fn tip(&self) -> BoxFuture<'_, Result<u64>> {
        let tip = self.source.current_tip();
        Box::pin(async move { Ok(tip) })
    }

    fn verify<'a>(
        &'a self,
        range: SlotRange,
        delivered: &'a HashMap<Signature, u64>,
        _thorough: bool,
    ) -> BoxFuture<'a, Result<Report>> {
        Box::pin(async move {
            let started = std::time::Instant::now();
            let fixture = self.source.fixture();
            let expected: HashMap<Signature, Landed> = fixture
                .transactions_in(range)
                .into_iter()
                .map(|(sig, slot, index)| (sig, Landed { slot, index }))
                .collect();
            let blocks = fixture.slots_in(range);
            let addresses = [self.program.clone()];
            let truth = GroundTruth {
                source: "fixture",
                addresses: &addresses,
                finalized_tip: self.source.current_tip().saturating_sub(FIXTURE_FINALITY),
                blocks: &blocks,
                expected: &expected,
            };
            Ok(build_report(
                range,
                truth,
                delivered,
                Vec::new(),
                &HashMap::new(),
                RpcStats::default(),
                started.elapsed().as_millis() as u64,
            ))
        })
    }

    fn repair<'a>(&'a self, _report: &'a mut Report) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
