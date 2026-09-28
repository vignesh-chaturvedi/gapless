# The 3-minute demo

Everything runs on live Solana mainnet through Solami. The point is to see a gap appear, get filled and get proven, rather than just hear about it. The wall clock (UTC) and slot numbers stay on screen the whole time, in the console's status line.

## Script

| Time | Beat | On screen | Say |
|---|---|---|---|
| 0:00 | The problem | Landing page, the live tape | "Your bot reconnects. Did it miss anything? Did it count something twice? Right now, you guess." |
| 0:15 | Live | **Open the live console**. The tape moves; the status line shows Solami's region and buffer | "Pump.fun on mainnet through Solami's Yellowstone gRPC. One bar per slot: green arrived live, blue is already verified against Solami's RPC." |
| 0:35 | Break it | Chaos panel: offline **30s**, **Kill**, **Confirm kill** | "Kill our stream through Solami's account API. The chain keeps going, so the gap grows on the tape." |
| 0:55 | Recover | The tape turns amber and catches up; the recovery loop fills in | "Back online, resumed with `from_slot` at the exact slot where it broke, duplicates dropped." |
| 1:15 | Prove it | Recovery loop: **Verified complete, N / N** | "Checked against `getTransactionsForAddress`: every expected transaction delivered, nothing counted twice." |
| 1:30 | The twist | Switch the **Handoff patch** off, **Kill** again (15s) | "Now the same outage without the handoff patch…" |
| 1:55 | The catch | **Verified, repaired: N missing**. **Details**: the missing transactions, cause *handoff*, Solscan links | "Solami dropped these at the switch back to live. The stream never showed a problem. Only verification caught them. This is why you verify." |
| 2:20 | Patch on | Switch the patch on, **Kill** again | "With the patch on, Gapless re-reads that switch itself, and it verifies complete." |
| 2:40 | Use it | **Incidents**, then **Integrate** | "Every outage recorded. A dozen lines put Gapless between Solami and your bot. Open source, `docker compose up`." |

Solami's handoff loss is intermittent: in our runs it hit 7 of 12 recoveries. If a patch-off take verifies complete, nothing was dropped that time, so break it again.

## Before recording

1. `.env` has `SOLAMI_API_KEY`. Nothing else is using the key's gRPC streams: the handoff patch needs the second one for a few seconds.
2. Start fresh, so incidents start at #1: `rm -f demo.db && cargo run -p gapless-server --release -- --db demo.db`. Build the console once first with `pnpm --dir console build`.
3. Open http://localhost:8790 at 1920×1080 (or 1440×900), in dark mode, with browser zoom at 100%.
4. Wait about a minute, until the tape shows blue (verified) bars behind the green ones.
5. The handoff patch should be **on** (the chaos panel switch).

## The recorded version

The published take is on [YouTube](https://youtu.be/LKObcNlrob0), filmed on 28 Sep 2026, 08:16–08:20 UTC.

[`tools/demo`](../tools/demo) films the same script automatically in headless Chrome. It uses real clicks on the real console against live mainnet, with captions and a visible cursor. Waiting (offline time, finalization) runs as a labelled 3× or 4× time-lapse; everything else is real time. A patch-off take in which Solami dropped nothing is cut and retried.

```bash
cd tools/demo && npm install
node record.mjs http://localhost:8790 take          # needs Chrome and a live gapless-server
node make-video.mjs take gapless-demo.mp4           # needs ffmpeg
```
