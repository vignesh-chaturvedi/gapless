# Verification evidence

Reports written by `gapless-verify run` against live mainnet (Pump.fun transactions, Solami gRPC and RPC), 2026-09-26.

| File | What happened | Result |
|---|---|---|
| `phase2-control-no-kill.json` | 2 min live, no disruption | 12,300 / 12,300, complete |
| `phase2-before-patch-run1.json` | killed through the account API, 60 s offline, replayed; no handoff patch yet | 4 missing, all in the handoff slot (target + 8) |
| `phase2-before-patch-run2.json` | same, repeated | 12 missing, all in the handoff slot (target + 7) |
| `phase2-kill-60s.json` | same disruption, with the handoff patch | 9,571 / 9,571, complete; the patch recovered 14 |

Each file has the whole-window report (per-slot counts, `getBlock` spot checks, verdict) and, for runs with a kill, a separate report for the incident's gap plus the handoff slots. The two "before patch" files predate per-transaction positions and causes; see `docs/spike.md` for that analysis.
