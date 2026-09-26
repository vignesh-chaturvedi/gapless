import { useFeed } from "@/lib/feed";
import type { SlotCell } from "@/lib/types";

/** What the tape needs each frame. */
export interface TapeFrame {
  tape: Map<number, SlotCell>;
  /** Latest processed slot on chain. */
  tip: number | null;
  highestComplete: number | null;
  verifiedThrough: number | null;
  /** `live`, `replaying` (recovering) or `down` (disconnected / holding). */
  stream: "live" | "replaying" | "down";
}

export type TapeSource = () => TapeFrame;

/** Read the live feed without re-rendering React. */
export const liveTape: TapeSource = () => {
  const { tape, snapshot, link } = useFeed.getState();
  const state = snapshot?.state.kind;
  const stream = link !== "open" ? "down" : state === "live" ? "live" : state === "replaying" ? "replaying" : "down";
  let high: number | null = snapshot?.metrics.highestComplete ?? null;
  for (const slot of tape.keys()) if (high === null || slot > high) high = slot;
  return {
    tape,
    tip: snapshot?.tip ?? high,
    highestComplete: high,
    verifiedThrough: snapshot?.verifiedThrough ?? null,
    stream,
  };
};
