# Verification evidence

Reports written by `gapless-verify run` and `gapless-server` against live mainnet (Pump.fun transactions, Solami gRPC and RPC), 2026-09-26.

| File | What happened | Result |
|---|---|---|
| `phase2-control-no-kill.json` | 2 min live, no disruption | 12,300 / 12,300, complete |
| `phase2-before-patch-run1.json` | killed through the account API, 60 s offline, replayed; no handoff patch yet | 4 missing, all in the handoff slot (target + 8) |
| `phase2-before-patch-run2.json` | same, repeated | 12 missing, all in the handoff slot (target + 7) |
| `phase2-kill-60s.json` | same disruption, with the handoff patch | 9,571 / 9,571, complete; the patch recovered 14 |
| `phase3-server-kill-60s.json` | `gapless-server` incident from `POST /api/chaos/kill {"holdSecs":60}` | 220-slot gap replayed, the patch recovered 21, 11,015 / 11,015 verified complete |
| `phase5-console-kill-30s.json` | Kill pressed in the console's chaos panel, 30 s offline | 112-slot gap, 3,185 replayed, 13 duplicates dropped, the patch recovered 10, 4,960 / 4,960 verified complete |
| `phase5-slow-consumer-drained.json` | Slow consumer (250 ms per update) until Solami's buffer filled; no listing watchdog yet | Solami closed the stream ~125 s in, but the client only saw the end-of-stream after its own buffers drained, minutes later at 4 updates/s. 618-slot gap, 2 replay steps, 14,036 / 14,036 verified complete |
| `phase5-slow-consumer-watchdog.json` | Same, with the listing watchdog | Gapless ended the stream once Solami stopped listing it; history confirmed `backpressure`. 486-slot gap, 2 replay steps, the patch recovered 14, 13,199 / 13,199 verified complete |

Each file has the whole-window report (per-slot counts, `getBlock` spot checks, verdict) and, for runs with a kill, a separate report for the incident's gap plus the handoff slots. The two "before patch" files predate per-transaction positions and causes; see `docs/spike.md` for that analysis.
