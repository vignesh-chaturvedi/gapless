# Phase 0 spike results

Run on 2026-09-26 against mainnet with a standard API key (Developer role, Pro trial) through `tools/probe` (`gapless-probe`). Every stream in these runs was served from the `ams` region. The program filter was Pump.fun (`6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`): successful, non-vote transactions only.

## Verdict

| # | Question | Answer | Go? |
|---|----------|--------|-----|
| 1 | Does the Solami SDK stream program transactions? | Yes. About 53–66 tx/s, no duplicates, p50 latency ~46 ms once streaming | Go |
| 2 | Does `from_slot` replay work? | Yes, but the horizon is **3,000 slots** (not 3,500), and a replay larger than ~450 slots trips backpressure | Go, with stepping |
| 3 | Can the API key call the account routes? | **Yes**, as `Authorization: Bearer <key>` or `x-api-key`. `?api_key=` returns 401 | Go |
| 4 | Can we kill our own stream? | Yes. `DELETE` returns `{"killed":true}` and the client gets `Cancelled: stream terminated by user` about 190 ms later | Go |
| 5 | Can we get ground truth for a gap cheaply? | `getBlock` is too heavy. **`getTransactionsForAddress`** with slot and status filters matches `getBlock` exactly at ~1% of the cost | Go |
| 6 | Does PAYG gRPC work on the Free plan? | Unanswered. PAYG gRPC is off on this account, and turning it on needs a Billing permission. The docs say gRPC starts at Pro | Ask Solami |

## 1. Live stream (`stream`)

- The subscribe call is accepted in ~1.4 s. The first update arrives ~3.5 s after connecting, as a short backlog burst.
- Throughput: 53–66 tx/s for Pump.fun. That is about 0.45–0.8 MB/s uncompressed, and the account API reported ~1.3 MB/s at peak.
- Latency (server `created_at` to receipt, clock skew included): p50 45.6 ms, p90 46.9 ms, p99 64 ms after the first 10 s. The multi-second p99 over a whole run comes only from the opening backlog.
- Each slot gets exactly one `SlotProcessed`, one `SlotConfirmed` and one `SlotFinalized` update. There are no interslot updates, because we don't request them.
- **No transaction ever arrived after its slot's `SlotProcessed` update**: 0 across 220 slots live, and 0 across 2,230 slots in the 10-minute capture. So `SlotProcessed(N)` means slot N is complete.
- Answering server pings with a ping-only `SubscribeRequest` is safe: transactions kept flowing after 7 pongs.
- Mainnet slot time is **~264 ms** (3.79 slots/s), not the 400 ms Solami's docs assume.

## 2. Replay (`replay`)

- `SubscribeReplayInfo` reports `first_available` = tip − ~3,000 slots, which is **about 13 minutes** at current slot times.
- Replay starts exactly at `from_slot`, arrives in slot order (0 regressions), and includes the slot status updates for the replayed slots.
- Catch-up speed is ~75 slots/s, or ~1,700 tx/s. A 100-slot gap caught up to live in 4.2 s.
- **A replay of 500 slots is closed by the server before it reaches live.** This happened every time: 4 runs, debug and release builds, with and without zstd. The connection history records `termination_reason: "backpressure"`. The replay backlog (~8,400–10,000 messages) overflows the 8,192-message `grpc_buffer_size`.
- **The client sees a clean end-of-stream, not the `RESOURCE_EXHAUSTED` status the troubleshooting docs describe.** Without asking the history API, a backpressure disconnect looks identical to a normal close.
- Asking for zstd-compressed responses (our own tonic client, since the SDK doesn't negotiate compression) got further (~10,000 transactions) but didn't prevent the close.

## 3. Account API (`connections`)

| Route | `?api_key=` | `Bearer <key>` | `x-api-key` |
|-------|-------------|----------------|-------------|
| `GET /auth/connections/grpc` | 401 | 200 | 200 |
| `GET /auth/connections` | 401 | 200 | 200 |
| `GET /auth/connections/grpc/history` | 401 | 200 | 200 |
| `GET /auth/grpc/usage` | 401 | 200 | 200 |
| `GET /auth/grpc-payg` | 401 | 200 | 200 |
| `GET /bandwidth` | — | — | 200 |
| `GET /auth/subscription` | — | — | 403 (needs BillingView) |

- Live connections report `conn_id`, `region`, `bytes_streamed`, `throughput_bps`, `buffer_size` and `buffer_pending`. Pending updates on 5-second ticks.
- History rows include `started_at`, `ended_at`, `bytes_streamed` and **`termination_reason`**. So far we've seen `backpressure` and `client_disconnect`.
- Limits on this account: 2 gRPC streams, 5 WebSocket connections, 200 RPC req/s, `sendTransaction` at 5 req/s.
- The trial's streaming bandwidth bucket (~1.28 TB, `source: plan`) **expires 2026-10-02 23:32 UTC (Oct 3, 05:02 IST)**. We can treat that as the trial end.

## 4. Kill (`kill`)

- `DELETE /auth/connections/grpc/{conn_id}` with the API key returns `{"killed":true}`. The Developer role includes StreamsKill.
- The client stream ends **~190 ms** later with `Cancelled: stream terminated by user`.
- We then resumed with `from_slot` = the last slot seen (not +1). That re-delivered **32 transactions we had already seen before the kill**. A naive consumer would count them twice, and a consumer that resumes at +1 would risk losing the rest of a slot cut off mid-stream.

## 5. Ground truth (`blocks` plus RPC experiments)

- `transactionDetails: "accounts"` is **rejected** (`-32602`).
- Mainnet carries **v1 transactions**, so `getBlock` needs `maxSupportedTransactionVersion: 1` (otherwise `-32015`).
- **Recent confirmed blocks come back with an empty transaction list and no error.** They fill in ~15 s later. Finalized blocks were complete in every test.
- Full JSON blocks are ~5 MB each. 150 blocks took 61–78 s and ~730–830 MB at concurrency 8–32, with no rate limiting. That's too slow for an on-camera check.
- 22–25% of matching transactions reference Pump.fun **only through an address lookup table**, so any filter replica has to include `meta.loadedAddresses`.
- `getSignaturesForAddress` matched `getBlock` exactly (271/271) but pages through failed transactions too, which are ~10× as many for Pump.fun. That took 15 pages for 60 slots.
- **`getTransactionsForAddress`** (Solami-only) with `transactionDetails: "signatures"`, `filters: { slot: { gte, lte }, status: "succeeded" }`, default descending sort, and `limit: 1000`, paged by `paginationToken`:
  - 11 slots: 174 signatures, **0 missing, 0 extra** against `getBlock`.
  - 151 slots: 4,150 signatures in 6 pages, 6 s, 930 KB.
  - `sortOrder: "asc"` starts from the deep archive (slot ~435M) and ignores the slot bound in practice, so don't use it.
  - Rows carry `slot` and `transactionIndex`.

## 6. PAYG gRPC on Free

`GET /auth/grpc-payg` → `{"enabled":false}`. Toggling it needs `PaygGrpcToggle`, which is in the Billing role, not Developer. The plans page says gRPC access starts at Pro. We can't test this without changing the plan, so it goes to Solami along with the trial-extension question.

## Fixtures

`gapless-probe record` writes frames of `[u64 LE receive-time nanos][length-delimited SubscribeUpdate]` to `fixtures/raw/` (gitignored). `gapless-probe inspect <file>` reads a capture back.

The Phase 0 capture, `fixtures/raw/capture-1790381350.bin`, holds 597 s of live mainnet: 273 MB, 53,069 frames, 46,319 unique transactions (77.6/s) across 2,231 slots, with every slot's Processed, Confirmed and Finalized updates. A trimmed version gets committed when offline mode lands.

## Decisions for the next phases

1. **Resume point.** A slot is complete when its `SlotProcessed` arrives. Resume from the lowest slot that has transactions but no `SlotProcessed` yet, otherwise from the highest completed slot + 1. Keep signature dedup anyway, because the kill test proved overlap happens.
2. **Replay stepping.** Gaps above ~450 Pump.fun slots will trip backpressure mid-replay. The supervisor keeps resuming from its cursor, with backoff, until it's live. Each step is recorded as part of the same incident. Replay horizon: 3,000 slots (~13 min). Beyond that, the gap is reported as unrecoverable by replay.
3. **Disconnect reasons.** Classify by the gRPC status when there is one (`Cancelled: stream terminated by user` means killed). Otherwise look up `termination_reason` in `/auth/connections/grpc/history`, because a clean end-of-stream can hide backpressure.
4. **Verifier.**
   - Main source of truth: `getTransactionsForAddress` over the incident's slot range, filtered to succeeded.
   - Independent spot check: `getBlock` (full, v1) on a few sampled slots, including lookup-table addresses.
   - Only verify slots at or below the finalized tip, and treat an empty block as not ready yet.
5. **Account API auth.** Send the API key as `Authorization: Bearer`. No session token needed.
6. **Compression.** Ask for zstd on our own tonic client. It cuts bytes on the wire and lets replay get further before backpressure.
7. **Buffer policy.** Consider raising `grpc_buffer_size` to 32,768 in the dashboard before recording the demo. Scenario C (`drop_new`) also needs a dashboard policy change, because the Developer role has only PolicyView.

## Open questions for Solami (@cryptociva)

- Can the Pro trial be extended through judging (Oct 28)? It currently ends Oct 2, 23:32 UTC.
- Does PAYG gRPC work on the Free plan?
- A backpressure disconnect reaches the client as a clean end-of-stream, while the docs say `RESOURCE_EXHAUSTED`. Is that intended?
- The replay horizon is 3,000 slots rather than the documented 3,500.
