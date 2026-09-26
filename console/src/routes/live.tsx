import { Flame } from "lucide-react";

import { ActivityLog } from "@/components/activity-log";
import { RowsSkeleton, ServerOffline } from "@/components/feed-states";
import { Panel, Readout } from "@/components/panel";
import { useCommandMenu } from "@/lib/command-menu";
import { SlotTape, TapeLegend } from "@/components/tape/slot-tape";
import { TxFeed } from "@/components/tx-feed";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";

const PROGRAMS: Record<string, string> = {
  "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P": "Pump.fun",
};

export function Live() {
  const link = useFeed((s) => s.link);
  const snapshot = useFeed((s) => s.snapshot);
  const setCommands = useCommandMenu((s) => s.setOpen);

  if (!snapshot && link === "closed") {
    return (
      <div className="mx-auto max-w-3xl px-4 py-10 lg:px-6">
        <ServerOffline />
      </div>
    );
  }

  const m = snapshot?.metrics;
  const program = snapshot?.program ?? "";
  const streaming = snapshot?.state.kind === "live" || snapshot?.state.kind === "replaying";
  // The server polls the chain tip even while the stream is down, so lag stays honest.
  const lag =
    snapshot?.tip != null && m?.highestComplete != null ? Math.max(0, snapshot.tip - m.highestComplete) : null;

  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 py-6 lg:px-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-[-0.02em]">Live</h1>
          <div className="mt-1 text-sm text-muted-foreground">
            {snapshot ? (
              <>
                Following <span className="text-foreground">{PROGRAMS[program] ?? "program"}</span>{" "}
                <span className="num text-xs" title={program}>
                  {fmt.short(program, 4)}
                </span>{" "}
                {snapshot.mode === "offline" ? "from a recorded mainnet fixture" : "on Solana mainnet through Solami"}
              </>
            ) : (
              <Skeleton className="h-4 w-72" />
            )}
          </div>
        </div>
        <Button variant="outline" size="sm" onClick={() => setCommands(true)}>
          <Flame aria-hidden="true" className="text-gap" />
          Break the stream
        </Button>
      </div>

      <Panel
        title="Slot tape"
        aside={
          <span className="num">
            verified to <span className="text-verified">{fmt.slot(snapshot?.verifiedThrough)}</span>
          </span>
        }
      >
        <SlotTape
          height={148}
          label={`Slot tape. Highest complete slot ${fmt.slot(m?.highestComplete)}, verified through ${fmt.slot(snapshot?.verifiedThrough)}.`}
        />
        <div className="border-t border-hairline px-4 py-2.5">
          <TapeLegend />
        </div>
      </Panel>

      <section aria-label="Stream readouts" className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-hairline bg-hairline sm:grid-cols-3 xl:grid-cols-6">
        {(
          [
            {
              label: "Throughput",
              value: !m ? fmt.NONE : streaming ? fmt.compact(m.txPerSec) : "0",
              unit: "tx/s",
              hint: streaming ? undefined : "stream is down",
            },
            {
              label: "Lag",
              value: lag === null ? fmt.NONE : fmt.int(lag),
              unit: "slots",
              tone: lag !== null && lag > 8 ? (streaming ? "replay" : "gap") : "default",
              hint: "behind the chain tip",
            },
            {
              label: "Latency",
              value: m?.latencyMs == null ? fmt.NONE : fmt.int(m.latencyMs),
              unit: "ms",
              hint: snapshot?.mode === "offline" ? "fixtures carry no timestamps" : "Solami stamp to receipt",
            },
            { label: "Delivered", value: fmt.compact(m?.delivered), hint: "each exactly once" },
            { label: "Duplicates dropped", value: fmt.int(m?.duplicates), hint: "re-sent after resumes" },
            { label: "Incidents", value: fmt.int(m?.incidents), hint: m ? fmt.plural(m.reconnects, "reconnect") : undefined },
          ] as const
        ).map((r) => (
          <div key={r.label} className="bg-panel px-4 py-3.5">
            {snapshot ? (
              <Readout
                label={r.label}
                value={r.value}
                unit={"unit" in r ? r.unit : undefined}
                hint={"hint" in r ? r.hint : undefined}
                tone={"tone" in r ? r.tone : "default"}
              />
            ) : (
              <div className="flex flex-col gap-2" aria-hidden="true">
                <Skeleton className="h-3 w-16" />
                <Skeleton className="h-6 w-20" />
              </div>
            )}
          </div>
        ))}
      </section>

      <div className="grid gap-4 lg:grid-cols-[1fr_1fr]">
        <Panel title="Transactions" aside={<span>sampled · newest first</span>} bodyClassName="max-h-[28rem] overflow-y-auto">
          {snapshot ? <TxFeed /> : <RowsSkeleton />}
        </Panel>
        <Panel title="Activity" bodyClassName="max-h-[28rem] overflow-y-auto">
          {snapshot ? <ActivityLog /> : <RowsSkeleton />}
        </Panel>
      </div>
    </div>
  );
}
