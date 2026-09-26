import { toast } from "sonner";

import { api } from "@/lib/api";

/** How slow "slow consumer" is: about 4 updates a second against Pump.fun's ~70. */
export const SLOW_CONSUMER_MS = 250;

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Kill our stream through Solami's account API, optionally staying offline to open a gap. */
export async function killStream(holdSecs?: number) {
  try {
    const result = await api.kill(holdSecs);
    const what =
      result.method === "kill"
        ? `Killed stream ${result.connId ?? ""} through Solami's account API`
        : "Cut the connection (offline mode has no account API)";
    toast(what, {
      description: holdSecs
        ? `Staying offline for ${holdSecs}s, then replaying the gap.`
        : "Reconnecting now and replaying from the last complete slot.",
    });
  } catch (error) {
    toast.error("Couldn't kill the stream", { description: reason(error) });
  }
}

/** Drop the connection from our side. */
export async function cutStream(holdSecs?: number) {
  try {
    await api.cut(holdSecs);
    toast("Cut the connection", {
      description: holdSecs ? `Staying offline for ${holdSecs}s, then replaying the gap.` : "Reconnecting now.",
    });
  } catch (error) {
    toast.error("Couldn't cut the connection", { description: reason(error) });
  }
}

/** Make the consumer slow (per-update delay) until Solami's buffer fills, or restore full speed. */
export async function setSlowConsumer(perUpdateMs: number | null) {
  try {
    await api.slow(perUpdateMs);
    toast(perUpdateMs ? `Consumer slowed to ${perUpdateMs} ms per update` : "Consumer back to full speed", {
      description: perUpdateMs
        ? "Watch Solami's buffer fill until it drops the stream for backpressure. Gapless restores full speed to recover."
        : undefined,
    });
  } catch (error) {
    toast.error("Couldn't change the consumer speed", { description: reason(error) });
  }
}

/** Turn the replay-to-live handoff patch on or off. */
export async function setHandoffPatch(enabled: boolean) {
  try {
    await api.patch(enabled);
    toast(enabled ? "Handoff patch on" : "Handoff patch off", {
      description: enabled
        ? "Recoveries re-read the slots around the switch to live."
        : "The next recovery won't re-read the handoff slots. Verification will show what Solami drops.",
    });
  } catch (error) {
    toast.error("Couldn't change the handoff patch", { description: reason(error) });
  }
}
