import type { Link } from "@/lib/feed";
import type { Snapshot } from "@/lib/types";

export type Tone = "live" | "replay" | "gap" | "idle";

export interface StreamStatus {
  tone: Tone;
  /** One or two words. */
  label: string;
  /** A short explanation for tooltips and screen readers. */
  detail: string;
}

/** What the stream is doing, in words, from the server link and the latest snapshot. */
export function streamStatus(link: Link, snapshot: Snapshot | null): StreamStatus {
  if (link === "connecting" && !snapshot) {
    return { tone: "idle", label: "Connecting", detail: "Connecting to gapless-server" };
  }
  if (link !== "open") {
    return { tone: "idle", label: "Server offline", detail: "gapless-server isn't reachable; retrying" };
  }
  if (!snapshot) return { tone: "idle", label: "Starting", detail: "Waiting for the first snapshot" };
  const s = snapshot.state;
  switch (s.kind) {
    case "live":
      return { tone: "live", label: "Live", detail: "Streaming from Solami, every slot accounted for" };
    case "replaying":
      return {
        tone: "replay",
        label: "Replaying",
        detail: `Replaying slots ${s.fromSlot.toLocaleString("en-US")} to ${s.targetSlot.toLocaleString("en-US")}`,
      };
    case "backoff":
      return s.delayMs >= 5_000
        ? { tone: "gap", label: "Offline", detail: `Held offline for ${Math.round(s.delayMs / 1000)}s to open a gap` }
        : { tone: "gap", label: "Reconnecting", detail: "The stream dropped; reconnecting" };
    case "connecting":
      return { tone: "replay", label: "Connecting", detail: `Opening the stream (attempt ${s.attempt})` };
    case "stopped":
      return { tone: "gap", label: "Stopped", detail: s.reason };
  }
}

export const toneText: Record<Tone, string> = {
  live: "text-live",
  replay: "text-replay",
  gap: "text-gap",
  idle: "text-muted-foreground",
};

export const toneBg: Record<Tone, string> = {
  live: "bg-live",
  replay: "bg-replay",
  gap: "bg-gap",
  idle: "bg-faint",
};
