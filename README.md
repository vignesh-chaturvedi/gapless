# Gapless

Prove your Solana stream didn't miss a thing.

Gapless runs on [Solami](https://solami.dev)'s Yellowstone gRPC. When the connection drops, it replays the missed slots with `from_slot`, removes duplicate transactions, and checks the recovered window against RPC `getBlock` to show nothing was lost.

> Work in progress. The build plan is in [`docs/plan.html`](docs/plan.html); Phase 0 findings are in [`docs/spike.md`](docs/spike.md).

## Prerequisites

- Rust (stable, via rustup) and `cmake` (the Solami SDK compiles `protoc` from source)
- A Solami standard API key with the Developer role

## Phase 0 probe

```bash
cp .env.example .env   # then paste your key into SOLAMI_API_KEY
cargo run -p gapless-probe -- stream --secs 30
```

Subcommands: `stream`, `replay`, `connections`, `kill`, `blocks`, `record`, `inspect`. Run with `--help` for options.

If `cargo` resolves to a Homebrew install, put rustup's toolchain first (`export PATH="$HOME/.cargo/bin:$PATH"`) so `rust-toolchain.toml` applies.

## License

MIT
