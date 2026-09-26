# gapless-server API

`gapless-server` listens on `http://127.0.0.1:8790` by default (`--listen` to change). Every field name is camelCase. CORS is open, so the console dev server can call it directly.

```bash
cargo run -p gapless-server --release                                            # live (needs SOLAMI_API_KEY)
cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst  # no key needed
```

In **offline** mode the server plays a recorded mainnet fixture in a loop. It supports the same `from_slot` replay, and the fixture serves as ground truth for verification and repair. The kill endpoint falls back to a client-side cut there. The fixture also emulates Solami's 8,192-message send buffer (and its backpressure close) and its handoff loss. `--horizon <slots>` shortens the replay horizon from 3,000.

## REST

| Method | Path | Returns |
|---|---|---|
| GET | `/api/health` | `{ ok, mode, sizes }`; `sizes` counts the engine's in-memory state (`delivered`, `dedup`, `ledger`, `trackedIncidents`, `symbols`, `covered`) for soak runs |
| GET | `/api/state` | [`Snapshot`](#snapshot) |
| GET | `/api/tape` | the last 600 [`SlotCell`](#slotcell)s |
| GET | `/api/incidents?limit=50` | [`Incident`](#incident)s, newest first, persisted in SQLite (`--db`, default `gapless.db`) |
| GET | `/api/incidents/{id}` | one [`Incident`](#incident) |
| POST | `/api/chaos/kill` | kill our stream through Solami's account API (`DELETE /auth/connections/grpc/{id}`) |
| POST | `/api/chaos/cut` | drop the connection from our side |
| POST | `/api/chaos/slow` | make the consumer slow on purpose |
| POST | `/api/chaos/patch` | turn the replay-to-live handoff patch on or off |

### Chaos bodies

```jsonc
// POST /api/chaos/kill  or  /api/chaos/cut
{ "holdSecs": 60 }        // optional: stay offline 60 s so there's a real gap to replay

// POST /api/chaos/slow
{ "perUpdateMs": 5 }      // sleep 5 ms per update until Solami's buffer fills; null or 0 restores full speed

// POST /api/chaos/patch
{ "enabled": false }
```

`kill` answers `{ ok, method: "kill", connId, killed }`, or `409` if our Solami connection isn't identified yet. The hold is released automatically when the incident recovers.

## WebSocket `/ws`

The server pushes JSON messages tagged by `type`. A client that falls behind receives a fresh `hello` instead of the messages it missed.

| `type` | When | Payload |
|---|---|---|
| `hello` | on connect | `snapshot`, `tape` (SlotCell[]), `incidents` (Incident[]), `txs` (Tx[], last 200), `log` (LogLine[]) |
| `batch` | every 100 ms when something changed | `slots` (newly completed SlotCells), `txs` (up to 40 sampled Tx), `txCount` (all delivered in the batch), `log` |
| `tick` | every second | `snapshot` |
| `incident` | whenever an incident changes | `incident` |
| `verified` | after rolling or incident verification | `range`, `slots` (SlotCells with `verified` set) |

## Types

### Snapshot

```jsonc
{
  "mode": "live",                      // or "offline"
  "program": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
  "startedAt": 1790389638000,          // unix ms
  "state": { "kind": "live" },         // State
  "stateSince": 1790389640000,         // unix ms the stream entered `state` (countdowns, "replaying for")
  "connId": "XMRwUb6crecD",            // our stream, as Solami's account API names it
  "metrics": {
    "updatesPerSec": 96.0, "txPerSec": 71.0, "highestComplete": 450540423, "tip": 450540423,
    "lagSlots": 0, "delivered": 2064, "duplicates": 0, "reconnects": 0, "incidents": 0,
    "latencyMs": 75.0, "dedupEntries": 2064
  },
  "tip": 450540423,                    // processed tip, polled every second (also during outages)
  "finalized": 450540391,              // finalized tip, which incident verification waits for
  "solami": {                          // our stream in Solami's account API; null until identified
    "connId": "XMRwUb6crecD", "region": "ams", "bytesStreamed": 9340000, "throughputBps": 481731,
    "bufferSize": 8192, "bufferPending": 0, "isPaygo": false, "liveStreams": 1, "sampledAt": 1790389660000,
    "emulated": false                  // offline: the fixture source's emulated buffer, not Solami
  },
  "verifiedThrough": 450540358,        // rolling verification has checked every slot up to here
  "controls": { "handoffPatch": true, "throttleMs": null, "holdSecs": null, "canKill": true },
  "indexer": { /* Indexer */ },
  "openIncident": null                 // the newest incident that isn't verified yet
}
```

### State

Tagged by `kind`:

- `connecting { attempt }`
- `replaying { incident, fromSlot, targetSlot }`
- `live`
- `backoff { delayMs, attempt }`
- `stopped { reason }`

### SlotCell

```jsonc
{
  "slot": 450540500,
  "origin": "live",          // "replay" if it came from a from_slot replay
  "complete": true,          // SlotProcessed arrived
  "txs": 22,                 // transactions delivered for this slot
  "verified": "ok",          // null until verified; then "ok", "missing" or "repaired"
  "missing": 0,
  "incident": null,          // incident id, for replayed slots
  "at": 1790389700000
}
```

### Tx

```jsonc
{
  "sig": "3Epr…Z1", "slot": 450509958,
  "origin": "live",          // "replay", "patch" (recovered by the handoff patch), or "duplicate"
                             // for a re-sent transaction Gapless dropped (sampled, up to 12 per batch)
  "kind": "buy",             // "sell", "create", "complete" (bonding curve done) or "other"
  "sol": 0.0886, "mint": "Fovb…pump", "symbol": "IBZ", "user": "1krn…KAJ",
  "at": 1790389394900
}
```

### Indexer

The sample consumer: Pump.fun's Anchor events (`TradeEvent`, `CreateEvent`, `CompleteEvent`), bucketed by minute using each event's own timestamp (offline, the fixture playback's clock, since a looping recording repeats its timestamps). `replayed` counts transactions that arrived through a replay or the handoff patch.

```jsonc
{
  "txs": 1525, "replayed": 312, "trades": 485, "buys": 256, "sells": 229, "buySol": 86.7, "sellSol": 79.0,
  "uniqueTraders": 309, "launches": 6, "graduations": 0,
  "minutes": [ { "minute": 29839689, "txs": 612, "replayed": 312, "trades": 201, "buys": 110, "sells": 91,
                 "buySol": 35.2, "sellSol": 30.1, "traders": 150, "launches": 3, "graduations": 0 } ],
  "recentLaunches": [ { "mint": "DQF9…pump", "name": "MONEYY", "symbol": "MONY", "at": 1790381351000 } ]
}
```

### Incident

From the first disconnect through replay, handoff patch and verification. `status` is `open`, then `recovered`, then `verified`.

```jsonc
{
  "id": 1,
  "status": "verified",
  "chaos": "killed through Solami's account API, then offline for 60s",
  "reason": { "code": "killed", "text": "killed through Solami's account API" },
  "detail": "stream terminated by user",
  "openedAt": 1790389682043, "recoveredAt": 1790389762016, "durationMs": 80000,
  "lastCompleteSlot": 450540474, "resumeFrom": 450540475,
  "gap": { "first": 450540475, "last": 450540694 },
  "unrecoverable": null,                          // slots already past Solami's 3,000-slot replay window
  "steps": [ { "attempt": 1, "fromSlot": 450540475, "targetSlot": 450540694, "startedAt": 1790389742220,
               "ended": null, "endedDetail": null, "transactions": 8139 } ],
  "replayed": 8139,
  "duplicates": 45,
  "naiveDoubleCounts": 45,                        // what a consumer without dedup would count twice
  "patch": { "slots": { "first": 450540694, "last": 450540714 }, "recovered": 21, "error": null },
  "verification": {
    "status": "done",                             // "pending" (waiting for finalization), "running", "failed" or "skipped"
    "range": { "first": 450540475, "last": 450540758 },   // the gap plus 64 handoff slots
    "error": null,
    "report": { /* VerificationReport */ }
  }
}
```

`reason.code` is one of `killed`, `backpressure`, `stream_limit`, `reconnect_limit`, `balance_exhausted`, `server_shutdown`, `backend_unavailable`, `backend_moved`, `session_expired`, `server_closed`, `stalled`, `cut`, `network`, `rejected`, `out_of_horizon` (a replay start that had already left the window; the next step starts further in), or Solami's own termination reason. A fatal `rejected` or `balance_exhausted` stops the stream without opening an incident. A bare end-of-stream is updated later from Solami's connection history.

### VerificationReport

This is `gapless_verify::Report`. It compares what the stream delivered with the expected set from Solami's `getTransactionsForAddress` (`source: "fixture"` offline). `getBlock` spot checks provide an independent second source.

```jsonc
{
  "range": { "first": 450540475, "last": 450540758 }, "finalizedTip": 450540800,
  "source": "getTransactionsForAddress", "addresses": ["6EF8…F6P"],
  "slotsWithBlocks": 284, "skippedSlots": 0,
  "expected": 11015, "delivered": 11015, "matched": 11015,
  "missing": [ { "signature": "…", "slot": 1, "position": 183, "cause": "replay_handoff", "incident": 1 } ],
  "repaired": [ { "signature": "…", "slot": 1, "blockTime": 1790389700, "feePayer": "…" } ],
  "orphaned": [], "landedElsewhere": [], "unexplained": [],
  "spotChecks": [ { "slot": 450540475, "hasBlock": true, "fromBlock": 38, "fromHistory": 38,
                    "viaLookupTable": 4, "agree": true, "onlyInBlock": [], "onlyInHistory": [] } ],
  "verdict": { "kind": "complete" },   // "incomplete" { missing }, "repaired" { repaired }, "inconclusive" { reason }
  "perSlot": [ { "slot": 450540475, "hasBlock": true, "expected": 38, "delivered": 38, "matched": 38 } ],
  "rpc": { "calls": 43, "bytes": 21198896, "retries": 0 },
  "elapsedMs": 40394
}
```

### LogLine

```jsonc
{ "at": 1790389762016, "level": "success", "text": "Recovered after 80.0s: …", "incident": 1 }
```

`level` is `info`, `warn`, `error` or `success`.
