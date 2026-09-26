import { m, type Variants } from "motion/react";
import { useRef, type ReactNode } from "react";

import { ActivityLog } from "@/components/activity-log";
import { RowsSkeleton, ServerOffline } from "@/components/feed-states";
import { BufferGauge } from "@/components/live/buffer-gauge";
import { ChaosPanel } from "@/components/live/chaos-panel";
import { IncidentTracker } from "@/components/live/incident-tracker";
import { IndexerPanel } from "@/components/live/indexer-panel";
import { StoppedBanner } from "@/components/live/stopped-banner";
import { Panel, Readout } from "@/components/panel";
import { RollingNumber } from "@/components/rolling-number";
import { SlotTape, TapeLegend } from "@/components/tape/slot-tape";
import { TransactionsPanel } from "@/components/tx-feed";
import { Skeleton } from "@/components/ui/skeleton";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import { useIncidentDrawer } from "@/lib/incident-drawer";

const PROGRAMS: Record<string, string> = {
  "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P": "Pump.fun",
};

// The console assembles itself once: panels rise in reading order, then hold still.
const stagger: Variants = { show: { transition: { staggerChildren: 0.05, delayChildren: 0.02 } } };
const rise: Variants = {
  hidden: { opacity: 0, y: 6 },
  show: { opacity: 1, y: 0, transition: { duration: 0.4, ease: [0.2, 0.8, 0.2, 1] } },
};

function Rise({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <m.div variants={rise} className={className}>
      {children}
    </m.div>
  );
}

function Readouts() {
  const snapshot = useFeed((s) => s.snapshot);
  const m = snapshot?.metrics;
  const streaming = snapshot?.state.kind === "live" || snapshot?.state.kind === "replaying";
  // The server polls the chain tip even while the stream is down, so lag stays honest.
  const lag =
    snapshot?.tip != null && m?.highestComplete != null ? Math.max(0, snapshot.tip - m.highestComplete) : null;
  const items = [
    {
      label: "Throughput",
      value: !m ? fmt.NONE : streaming ? fmt.compact(m.txPerSec) : "0",
      unit: "tx/s",
      hint: streaming ? `${fmt.compact(m?.updatesPerSec)} updates/s` : "stream is down",
    },
    {
      label: "Lag",
      value: lag === null ? fmt.NONE : fmt.int(lag),
      unit: lag === 1 ? "slot" : "slots",
      tone: lag !== null && lag > 8 ? (streaming ? "replay" : "gap") : "default",
      hint: "behind the chain tip",
    },
    {
      label: "Latency",
      // A slow consumer pushes latency into minutes; switch units rather than print 107,828 ms.
      value: m?.latencyMs == null ? fmt.NONE : m.latencyMs < 10_000 ? fmt.int(m.latencyMs) : fmt.duration(m.latencyMs),
      unit: m?.latencyMs != null && m.latencyMs >= 10_000 ? undefined : "ms",
      tone: m?.latencyMs != null && m.latencyMs >= 10_000 ? "replay" : "default",
      hint: snapshot?.mode === "offline" ? "fixtures carry no timestamps" : "Solami stamp to receipt",
    },
    { label: "Delivered", value: fmt.int(m?.delivered), hint: "each exactly once" },
    { label: "Duplicates dropped", value: fmt.int(m?.duplicates), hint: "sent twice by the stream" },
    {
      label: "Incidents",
      value: fmt.int(m?.incidents),
      hint: m ? fmt.plural(m.reconnects, "reconnect") : undefined,
    },
  ] as const;

  return (
    <section
      aria-label="Stream readouts"
      className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-hairline bg-hairline sm:grid-cols-3 xl:grid-cols-6"
    >
      {items.map((r) => (
        <div key={r.label} className="bg-panel px-4 py-3.5">
          {snapshot ? (
            <Readout
              label={r.label}
              value={r.value === fmt.NONE ? r.value : <RollingNumber value={r.value} />}
              unit={"unit" in r ? r.unit : undefined}
              hint={r.hint}
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
  );
}

export function Live() {
  const link = useFeed((s) => s.link);
  const snapshot = useFeed((s) => s.snapshot);
  const drawer = useIncidentDrawer();
  const tapeEmpty = useFeed((s) => s.tape.size === 0);
  const chaosRef = useRef<HTMLDivElement>(null);

  if (!snapshot && link === "closed") {
    return (
      <div className="mx-auto max-w-3xl px-4 py-10 lg:px-6">
        <ServerOffline />
      </div>
    );
  }

  const metrics = snapshot?.metrics;
  const program = snapshot?.program ?? "";
  const focusChaos = () => {
    chaosRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
    chaosRef.current?.querySelector<HTMLElement>("input, button")?.focus({ preventScroll: true });
  };

  return (
    <m.div
      initial="hidden"
      animate="show"
      variants={stagger}
      className="mx-auto flex max-w-[1600px] flex-col gap-4 px-4 py-6 lg:px-6"
    >
      <Rise>
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
      </Rise>

      {snapshot?.state.kind === "stopped" && (
        <Rise>
          <StoppedBanner reason={snapshot.state.reason} />
        </Rise>
      )}

      <Rise>
        <Panel
          title="Slot tape"
          aside={
            <span className="num">
              verified to <span className="text-verified">{fmt.slot(snapshot?.verifiedThrough)}</span>
            </span>
          }
        >
          <div className="relative">
            <SlotTape
              height={156}
              onSelectIncident={drawer.open}
              label={`Slot tape. Highest complete slot ${fmt.slot(metrics?.highestComplete)}, verified through ${fmt.slot(snapshot?.verifiedThrough)}.`}
            />
            {tapeEmpty && snapshot && (
              <p className="pointer-events-none absolute inset-0 flex items-center justify-center text-sm text-muted-foreground">
                {snapshot.state.kind === "stopped" ? "No slots: the stream never started." : "Waiting for the first slot…"}
              </p>
            )}
          </div>
          <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline px-4 py-2.5">
            <TapeLegend />
            <span className="hidden text-xs text-faint md:inline">One bar per slot, newest on the right. Hover to inspect.</span>
          </div>
        </Panel>
      </Rise>

      <Rise>
        <Readouts />
      </Rise>

      <div className="grid items-start gap-4 xl:grid-cols-[minmax(0,1fr)_24rem]">
        <div className="flex min-w-0 flex-col gap-4">
          <Rise>{snapshot ? <IncidentTracker onBreak={focusChaos} /> : <Skeleton className="h-52 rounded-lg" />}</Rise>
          <Rise>
            <IndexerPanel />
          </Rise>
        </div>
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-1">
          <Rise>
            <div ref={chaosRef}>{snapshot ? <ChaosPanel /> : <Skeleton className="h-96 rounded-lg" />}</div>
          </Rise>
          <Rise>
            <BufferGauge />
          </Rise>
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Rise className="min-w-0">
          <TransactionsPanel />
        </Rise>
        <Rise className="min-w-0">
          <Panel title="Activity" aside={<span className="hidden sm:inline">what Gapless did, and why</span>} bodyClassName="h-[28.5rem] overflow-y-auto">
            {snapshot ? <ActivityLog /> : <RowsSkeleton />}
          </Panel>
        </Rise>
      </div>
    </m.div>
  );
}
