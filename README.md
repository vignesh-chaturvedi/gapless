# Gapless

Prove your Solana stream didn't miss a thing.

Gapless runs on [Solami](https://solami.dev)'s Yellowstone gRPC. When the connection drops, it replays the missed slots with `from_slot`, removes duplicate transactions, and checks the recovered window against RPC `getBlock` to show nothing was lost.

> Work in progress. The build plan is in [`docs/plan.html`](docs/plan.html); Phase 0 findings are in [`docs/spike.md`](docs/spike.md).

## Prerequisites

- Rust (stable, via rustup) and `cmake` (the Solami SDK compiles `protoc` from source)
- A Solami standard API key with the Developer role

## The `gapless` crate

`crates/gapless` wraps a Solami transaction subscription and keeps it whole across disconnects:

- **Resume point:** it tracks slot completeness (a slot is complete once its `SlotProcessed` arrives) and resumes from the lowest incomplete slot with `from_slot`.
- **Deep replays:** when Solami closes a replay for backpressure, it keeps resuming from its cursor until it's live again.
- **Duplicates:** it drops re-sent transactions by signature.
- **Disconnect reasons:** it takes them from the gRPC status, or from Solami's connection history when the stream just ends.

```rust
use futures::StreamExt;

let (mut events, control) = gapless::Gapless::builder(api_key)
    .program("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
    .build()?
    .start();
while let Some(event) = events.next().await {
    match event {
        gapless::Event::Transaction(tx) => { /* each transaction exactly once */ }
        gapless::Event::Recovered(incident) => { /* gap, replay steps, duplicates dropped */ }
        _ => {}
    }
}
```

Watch it live, then kill its stream (dashboard or `DELETE /auth/connections/grpc/{id}`) to see it recover:

```bash
GAPLESS_HOLD_SECS=60 cargo run -p gapless --example tail --release
```

`GAPLESS_HOLD_SECS` keeps the client offline that long after the drop, so the kill becomes a real outage to replay.

## Verification (`gapless-verify`)

`crates/gapless-verify` rebuilds what a filter *should* have delivered for a slot range and compares it with what the stream *did* deliver:

- **Expected set:** Solami's `getTransactionsForAddress` with `filters: { slot: { gte, lte }, status: "succeeded" }`.
- **Independent check:** a few full blocks from `getBlock`, with our own filter applied, including lookup-table addresses.
- **Differences:** every one is classified as missing, orphaned (a dead fork), landed in another slot, or unexplained. Missing transactions carry their block position and likely cause, and can be repaired with `getTransaction`.

```bash
# stream 2 minutes, kill the stream after 20 s, stay offline 60 s, recover, then verify twice
cargo run -p gapless-verify --release -- run --secs 120 --kill-after 20 --hold 60 --out report.json

# check the two ground truths against each other, slot by slot
cargo run -p gapless-verify --release -- sources --slots 20
```

Verification found that Solami drops the start of the slot where a resumed stream switches from replay to live. Gapless now re-reads those slots on a short second stream (the "handoff patch") and delivers what was dropped. The before and after reports are in [`docs/evidence/`](docs/evidence/).

## Phase 0 probe

```bash
cp .env.example .env   # then paste your key into SOLAMI_API_KEY
cargo run -p gapless-probe -- stream --secs 30
```

Subcommands: `stream`, `replay`, `connections`, `kill`, `blocks`, `record`, `inspect`. Run with `--help` for options.

If `cargo` resolves to a Homebrew install, put rustup's toolchain first (`export PATH="$HOME/.cargo/bin:$PATH"`) so `rust-toolchain.toml` applies.

## License

MIT
