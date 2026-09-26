# Soak run

`gapless-server` live on mainnet (Pump.fun through Solami) for 125 minutes on 2026-09-26, sampled once a minute by `scripts/soak.sh`. The chaos scenarios (`scripts/scenarios.sh`, A–D twice) ran during the first 55 minutes, including two 15-minute outages. The rest was an undisturbed live stream.

| | |
|---|---|
| Memory (RSS) | 19 MB at start, 77 MB peak (holding a deep replay's signatures for verification), 69 MB at the end |
| Delivered | 448,531 transactions, each exactly once |
| Duplicates dropped | 107, all during replays |
| Incidents / reconnects | 12 / 24, all from the scenarios (deep replays take several steps); none while undisturbed |
| Dedup window | 71,496 signatures at most (3,512 slots), 68,622 at the end |
| Tape (ledger) | capped at 3,001 slots |
| Incidents in memory | at most 11 (unverified plus the 10 newest) |
| Live and caught up | 81 of 126 samples (lag ≤ 8 slots); the rest were the deliberate outages |

Each structure stays flat. The dedup window follows traffic, 57k–71k signatures at 55–90 tx/s. Resident memory on macOS swings minute to minute (14–77 MB). In the undisturbed hour it averaged 57–63 MB per 12-minute window, with no upward trend beyond the dedup window's own growth with traffic. Most of the swing was each 10-second rolling check copying the whole delivered-signature map (~70k entries). Since this run, a check gets only the signatures within 256 slots of its range.

Every 10 minutes:

| UTC | RSS MB | State | Lag | tx/s | Delivered | Dupes | Reconnects | Incidents | Dedup | Delivered map | Tape | Verified behind |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 10:53:40 | 19.3 | live | 1 | 46 | 1,180 | 0 | 0 | 0 | 1,180 | 1,201 | 77 | 40 |
| 11:03:41 | 21.0 | backoff | 398 | 35 | 25,870 | 31 | 5 | 4 | 25,870 | 25,884 | 1,931 | -2 |
| 11:13:41 | 18.8 | backoff | 2653 | 35 | 25,870 | 31 | 5 | 4 | 25,870 | 25,884 | 1,931 | -2 |
| 11:23:42 | 42.1 | live | 2 | 48 | 87,751 | 62 | 13 | 7 | 48,501 | 61,931 | 3,001 | 873 |
| 11:33:42 | 28.9 | backoff | 1183 | 29 | 101,450 | 87 | 17 | 11 | 46,855 | 54,017 | 3,001 | -1 |
| 11:43:43 | 45.0 | replaying | 1853 | 572 | 121,137 | 87 | 20 | 12 | 45,441 | 73,698 | 3,001 | 1586 |
| 11:53:43 | 48.6 | live | 0 | 108 | 184,547 | 107 | 24 | 12 | 55,688 | 62,443 | 3,000 | 50 |
| 12:03:44 | 46.5 | live | 2 | 68 | 222,439 | 107 | 24 | 12 | 57,643 | 64,788 | 3,001 | 56 |
| 12:13:45 | 46.4 | live | 0 | 61 | 258,254 | 107 | 24 | 12 | 57,637 | 65,867 | 3,001 | 59 |
| 12:23:46 | 69.3 | live | 0 | 45 | 296,355 | 107 | 24 | 12 | 57,880 | 68,199 | 3,001 | 59 |
| 12:33:46 | 44.5 | live | 0 | 279 | 337,073 | 107 | 24 | 12 | 61,454 | 71,802 | 3,001 | 60 |
| 12:43:47 | 67.4 | live | 1 | 98 | 382,979 | 107 | 24 | 12 | 67,335 | 79,018 | 3,001 | 64 |
| 12:53:48 | 61.3 | live | 2 | 120 | 425,667 | 107 | 24 | 12 | 70,990 | 81,172 | 3,001 | 69 |
| 12:58:48 | 69.3 | live | 0 | 69 | 448,531 | 107 | 24 | 12 | 68,622 | 81,634 | 3,000 | 69 |
