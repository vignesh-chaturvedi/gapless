import { Flame, Scissors, Snail, Wrench } from "lucide-react";
import { useState, type ReactNode } from "react";

import { ConfirmButton } from "@/components/confirm-button";
import { Panel } from "@/components/panel";
import { Segmented } from "@/components/segmented";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { SLOW_CONSUMER_MS, cutStream, killStream, setHandoffPatch, setSlowConsumer } from "@/lib/chaos";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import type { Snapshot } from "@/lib/types";
import { useNow } from "@/lib/use-now";
import { cn } from "@/lib/utils";

const HOLDS = [
  { value: 0, label: "None" },
  { value: 15, label: "15s" },
  { value: 30, label: "30s" },
  { value: 60, label: "60s" },
];

function Action({
  icon,
  title,
  children,
  control,
  htmlFor,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
  control: ReactNode;
  htmlFor?: string;
}) {
  const Title = htmlFor ? "label" : "p";
  return (
    <div className="grid grid-cols-[1.25rem_1fr_auto] items-start gap-x-3 py-3 first:pt-0 last:pb-0">
      <span aria-hidden="true" className="mt-0.5 text-muted-foreground [&_svg]:size-4">
        {icon}
      </span>
      <div className="min-w-0">
        <Title htmlFor={htmlFor} className="block text-sm font-medium">
          {title}
        </Title>
        <p className="mt-0.5 text-xs text-pretty text-muted-foreground">{children}</p>
      </div>
      <div className="flex items-center self-center">{control}</div>
    </div>
  );
}

/** What the stream is doing right now, in the chaos panel's words. */
function StreamLine({ snapshot, now, waitMs }: { snapshot: Snapshot; now: number; waitMs: number }) {
  const s = snapshot.state;
  let tone: "live" | "replay" | "gap" | "idle" = "idle";
  let text: ReactNode = "Connecting…";
  if (s.kind === "live" && snapshot.controls.throttleMs) {
    tone = "replay";
    text = `Slow consumer: ${snapshot.controls.throttleMs} ms per update. Solami's buffer is filling.`;
  } else if (s.kind === "live" && waitMs > 0) {
    tone = "live";
    text = (
      <>
        Live. The next outage opens in <span className="num text-foreground">{fmt.duration(waitMs)}</span>
      </>
    );
  } else if (s.kind === "live") {
    tone = "live";
    text = "Live. Ready to break.";
  } else if (s.kind === "backoff") {
    tone = "gap";
    const left = Math.max(0, snapshot.stateSince + s.delayMs - now);
    text = (
      <>
        Offline. Reconnecting in <span className="num text-foreground">{fmt.duration(left)}</span>
      </>
    );
  } else if (s.kind === "replaying") {
    tone = "replay";
    text = (
      <>
        Replaying slots <span className="num text-foreground">{fmt.slot(s.fromSlot)}</span>–
        <span className="num text-foreground">{fmt.slot(s.targetSlot)}</span>
      </>
    );
  } else if (s.kind === "stopped") {
    tone = "gap";
    text = `Stopped: ${s.reason}`;
  }
  return (
    <p role="status" aria-live="polite" className="flex items-center gap-2 text-xs text-muted-foreground">
      <span
        aria-hidden="true"
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          tone === "live" ? "bg-live" : tone === "replay" ? "bg-replay" : tone === "gap" ? "bg-gap" : "bg-faint",
        )}
      />
      <span className="min-w-0">{text}</span>
    </p>
  );
}

/** Break the stream on purpose: kill it through Solami, cut it, or make the consumer slow. */
export function ChaosPanel({ id }: { id?: string }) {
  const snapshot = useFeed((s) => s.snapshot);
  const slotMs = useFeed((s) => s.slotMs);
  const now = useNow(500);
  const [hold, setHold] = useState(30);

  if (!snapshot) return null;
  const { controls, state, mode } = snapshot;
  const offline = mode === "offline";
  const live = state.kind === "live";
  const connected = live || state.kind === "replaying";
  const slow = controls.throttleMs != null;
  const holds = controls.maxHoldSecs != null ? HOLDS.filter((h) => h.value <= controls.maxHoldSecs!) : HOLDS;
  const held = holds.some((h) => h.value === hold) ? hold : holds[holds.length - 1].value;
  const holdSecs = held || undefined;
  const gapSlots = held && slotMs ? Math.round((held * 1000) / slotMs) : null;
  const waitMs = controls.nextOutageAt != null ? Math.max(0, controls.nextOutageAt - now) : 0;
  const cooling = waitMs > 0 ? `Public demo: the next outage is allowed in ${fmt.duration(waitMs)}` : undefined;
  const publicOff = controls.public ? "Off on this public demo; run Gapless locally to try it" : undefined;

  const killBlocked = !live
    ? "The stream isn't live right now"
    : cooling
      ? cooling
      : !offline && !controls.canKill
        ? "Waiting for Solami to list our stream"
        : undefined;

  return (
    <Panel id={id} title="Chaos" aside={<span className="hidden sm:inline">break it on purpose</span>} bodyClassName="flex flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <div>
          <p className="text-sm font-medium">Stay offline after the drop</p>
          <p className="text-xs text-muted-foreground">
            {held ? (
              <>
                Opens a gap of about <span className="num">{gapSlots ? fmt.int(gapSlots) : "--"}</span> slots to replay.
              </>
            ) : (
              "Reconnect at once; the gap is only the reconnect time."
            )}
          </p>
        </div>
        <Segmented label="Stay offline for" value={held} options={holds} onChange={setHold} />
      </div>

      {controls.public && (
        <p className="rounded-md border border-hairline bg-muted/40 px-3 py-2 text-xs text-pretty text-muted-foreground">
          {cooling ? (
            <>
              Public demo: one outage every 5 minutes, for everyone. Next one in{" "}
              <span className="num text-foreground">{fmt.duration(waitMs)}</span>.
            </>
          ) : (
            "Public demo: one outage every 5 minutes, for everyone, up to 30 s offline. Go ahead and break it."
          )}
        </p>
      )}

      <div className="divide-y divide-hairline border-t border-hairline pt-3">
        <Action
          icon={<Flame />}
          title="Kill via Solami"
          control={
            <ConfirmButton
              confirm="Confirm kill"
              disabled={Boolean(killBlocked)}
              disabledReason={killBlocked}
              onConfirm={() => killStream(holdSecs)}
            >
              Kill
            </ConfirmButton>
          }
        >
          {offline ? (
            "Offline there's no account API, so this cuts the connection instead."
          ) : (
            <>
              Ends our stream{" "}
              {snapshot.connId && (
                <span className="num" title={snapshot.connId}>
                  {fmt.short(snapshot.connId, 4)}
                </span>
              )}{" "}
              through Solami's account API, as an operator would.
            </>
          )}
        </Action>
        <Action
          icon={<Scissors />}
          title="Cut the connection"
          control={
            <ConfirmButton
              confirm="Confirm cut"
              disabled={!connected || Boolean(cooling)}
              disabledReason={cooling ?? "The stream isn't connected right now"}
              onConfirm={() => cutStream(holdSecs)}
            >
              Cut
            </ConfirmButton>
          }
        >
          Drops the gRPC connection from our side, like a network blip.
        </Action>
        <Action
          icon={<Snail />}
          title="Slow consumer"
          control={
            slow ? (
              <Button variant="outline" size="sm" className="min-w-24 pointer-coarse:h-10" onClick={() => setSlowConsumer(null)}>
                Restore
              </Button>
            ) : (
              <ConfirmButton
                confirm="Confirm slow"
                disabled={!live || Boolean(publicOff)}
                disabledReason={publicOff ?? "The stream isn't live right now"}
                onConfirm={() => setSlowConsumer(SLOW_CONSUMER_MS)}
              >
                Slow down
              </ConfirmButton>
            )
          }
        >
          Reads one update every <span className="num">{SLOW_CONSUMER_MS}</span> ms until Solami's{" "}
          {offline ? "(emulated) " : ""}buffer fills and it drops us for backpressure.
        </Action>
        <Action
          icon={<Wrench />}
          title="Handoff patch"
          htmlFor="handoff-patch"
          control={
            <Switch
              id="handoff-patch"
              checked={controls.handoffPatch}
              disabled={controls.public}
              title={publicOff}
              onCheckedChange={(on) => setHandoffPatch(on)}
              className="data-checked:bg-verified"
            />
          }
        >
          After each replay, re-reads 20 slots to recover what Solami drops at the switch back to live.
        </Action>
      </div>

      <div className="border-t border-hairline pt-3">
        <StreamLine snapshot={snapshot} now={now} waitMs={waitMs} />
      </div>
    </Panel>
  );
}
