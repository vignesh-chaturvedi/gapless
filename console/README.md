# Gapless console

The web console for `gapless-server`. It has:

- a live slot tape (canvas) with a hover inspector and incident brackets
- rolling stream readouts
- a recovery loop that follows the newest incident live: disconnected, replayed, handoff patched, verified
- a chaos panel: kill through Solami, cut, slow consumer (each confirmed with a second press), and the handoff patch switch
- Solami's send buffer for our stream, with its trend and time to backpressure
- a Pump.fun indexer panel whose per-minute chart hatches the transactions recovered by replay
- a virtualized transaction feed (live, replayed, patched and dropped duplicates) that pauses on hover, plus an activity log
- incident details in a drawer (`?incident=<id>`) or on their own page, with a per-slot verification strip
- a command palette (⌘K)

Vite, React 19, TypeScript, Tailwind v4, shadcn/ui (Radix), Motion, zustand, TanStack Query and TanStack Virtual. The design direction and tokens are in [`../brand.md`](../brand.md).

```bash
# 1. start the server (live needs SOLAMI_API_KEY in ../.env; --offline needs nothing)
cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst

# 2. start the console; it proxies /api and /ws to 127.0.0.1:8790
pnpm install
pnpm --dir console dev
```

Set `GAPLESS_SERVER` to point the dev proxy at another server. `pnpm --dir console build` writes a static bundle to `console/dist/`.
