# Gapless console

The web console for `gapless-server`. It has:

- a live slot tape
- stream readouts, including Solami's own buffer telemetry
- a transaction feed and activity log
- incidents with replay, handoff patch and verification timelines
- a command palette (⌘K) for breaking the stream on purpose

Vite, React 19, TypeScript, Tailwind v4, shadcn/ui (Radix), zustand and TanStack Query. The design direction and tokens are in [`../brand.md`](../brand.md).

```bash
# 1. start the server (live needs SOLAMI_API_KEY in ../.env; --offline needs nothing)
cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst

# 2. start the console; it proxies /api and /ws to 127.0.0.1:8790
pnpm install
pnpm --dir console dev
```

Set `GAPLESS_SERVER` to point the dev proxy at another server. `pnpm --dir console build` writes a static bundle to `console/dist/`.
