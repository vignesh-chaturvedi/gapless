import type { TapeFrame, TapeSource } from "@/lib/tape";
import type { SlotCell } from "@/lib/types";

const SLOT_MS = 400;

/**
 * A simulated tape for when gapless-server isn't running: live slots, an outage, the replay
 * catching up, then verification sweeping through. Always labelled as a preview in the UI.
 */
export function createPreviewTape(): TapeSource {
  const start = performance.now();
  const base = 450_000_000;
  const tape = new Map<number, SlotCell>();
  const cycle = 90; // slots per loop: 40 live, 14 down, then replay and verify
  let lastSlot = base;

  const txsFor = (slot: number) => 8 + ((slot * 2654435761) % 997) % 40;

  return (): TapeFrame => {
    const now = performance.now();
    const tip = base + Math.floor((now - start) / SLOT_MS);
    const phase = (tip - base) % cycle;
    const outageStart = tip - phase + 40;
    const down = phase >= 40 && phase < 54;
    const replaying = phase >= 54 && phase < 58;

    // Stream everything up to the tip, except during the outage.
    for (let slot = lastSlot + 1; slot <= tip; slot++) {
      const p = (slot - base) % cycle;
      if (p >= 40 && p < 54) continue;
      tape.set(slot, { slot, origin: "live", complete: true, txs: txsFor(slot), verified: null, missing: 0, incident: null, at: now });
    }
    lastSlot = tip;
    // Replay fills the outage.
    if (!down) {
      for (let slot = outageStart; slot < outageStart + 14 && slot <= tip; slot++) {
        if (!tape.has(slot)) {
          tape.set(slot, { slot, origin: "replay", complete: true, txs: txsFor(slot), verified: null, missing: 0, incident: 1, at: now });
        }
      }
    }
    // Verification trails the tip by ~30 slots.
    const verifiedThrough = tip - 30;
    for (const cell of tape.values()) {
      if (cell.slot <= verifiedThrough && !cell.verified) cell.verified = "ok";
    }
    for (const slot of [...tape.keys()]) if (slot < tip - 400) tape.delete(slot);

    let highestComplete = 0;
    for (const slot of tape.keys()) if (slot > highestComplete) highestComplete = slot;
    return {
      tape,
      tip,
      highestComplete: down ? outageStart - 1 : highestComplete,
      verifiedThrough,
      stream: down ? "down" : replaying ? "replaying" : "live",
    };
  };
}
