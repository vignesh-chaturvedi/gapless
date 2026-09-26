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

## Phase 0 probe

```bash
cp .env.example .env   # then paste your key into SOLAMI_API_KEY
cargo run -p gapless-probe -- stream --secs 30
```

Subcommands: `stream`, `replay`, `connections`, `kill`, `blocks`, `record`, `inspect`. Run with `--help` for options.

If `cargo` resolves to a Homebrew install, put rustup's toolchain first (`export PATH="$HOME/.cargo/bin:$PATH"`) so `rust-toolchain.toml` applies.

## License

MIT
