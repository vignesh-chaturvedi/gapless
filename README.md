# Gapless

Prove your Solana stream didn't miss a thing.

Gapless runs on [Solami](https://solami.dev)'s Yellowstone gRPC. When the connection drops, it replays the missed slots with `from_slot`, removes duplicate transactions, and checks the recovered window against RPC `getBlock` to show nothing was lost.

> Work in progress. Findings about Solami's stream behaviour are in [`docs/spike.md`](docs/spike.md); verification evidence is in [`docs/evidence/`](docs/evidence/).

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

## The server (`gapless-server`)

`crates/gapless-server` runs Gapless and serves the console:

- **Live feed:** a WebSocket slot tape, sampled transactions and an activity log.
- **Pump.fun indexer:** trades, buy/sell volume, launches and graduations, decoded from Pump.fun's Anchor events.
- **Solami stream telemetry:** buffer and throughput, from the account API.
- **Rolling verification** of everything delivered.
- **Chaos endpoints:** kill through Solami's API, client cut, slow consumer, handoff patch on/off.
- **Incident history:** every outage is replayed, patched, verified automatically and stored in SQLite.

```bash
cargo run -p gapless-server --release                                             # live on :8790 (needs SOLAMI_API_KEY)
cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst   # no key: plays recorded mainnet

curl -X POST localhost:8790/api/chaos/kill -H 'content-type: application/json' -d '{"holdSecs":60}'
curl localhost:8790/api/incidents?limit=1
```

The API and WebSocket contract is in [`docs/api.md`](docs/api.md). `fixtures/pumpfun-150s.bin.zst` holds 150 s of real Pump.fun traffic (3.6 MB). Offline mode loops it seamlessly, with `from_slot` replay, and uses it as verification ground truth.

## The console

`console/` is the web UI (Vite, React, Tailwind, shadcn/ui). The direction is set in [`brand.md`](brand.md): an instrument-grade "flight recorder". Its surfaces are graphite, and the only colours are the four slot states: live, replayed, gap and verified.

```bash
cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst   # or live, with a key
pnpm install && pnpm --dir console dev                                           # http://localhost:5173
```

Press ⌘K for commands, including breaking the stream on purpose.

## Phase 0 probe

```bash
cp .env.example .env   # then paste your key into SOLAMI_API_KEY
cargo run -p gapless-probe -- stream --secs 30
```

Subcommands: `stream`, `replay`, `connections`, `kill`, `blocks`, `record`, `inspect`, `analyze`, `events`, `trim`, `unary`. Run with `--help` for options.

If `cargo` resolves to a Homebrew install, put rustup's toolchain first (`export PATH="$HOME/.cargo/bin:$PATH"`) so `rust-toolchain.toml` applies.

## License

MIT
