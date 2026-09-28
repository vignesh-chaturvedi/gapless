import type { ReactNode } from "react";

import * as fmt from "@/lib/format";
import { useFeed } from "@/lib/feed";
import { useNow } from "@/lib/use-now";
import { cn } from "@/lib/utils";

function Item({ term, children, title }: { term: string; children: ReactNode; title?: string }) {
  return (
    <div className="flex shrink-0 items-center gap-1.5 border-r border-hairline px-3 last:border-r-0" title={title}>
      <dt className="text-faint">{term}</dt>
      <dd className="num text-foreground">{children}</dd>
    </div>
  );
}

/** The telemetry strip along the bottom of the console, like an instrument's status line. */
export function StatusBar() {
  const link = useFeed((s) => s.link);
  const snapshot = useFeed((s) => s.snapshot);
  const m = snapshot?.metrics;
  const solami = snapshot?.solami;
  const now = useNow();
  const lag =
    snapshot?.tip != null && m?.highestComplete != null ? Math.max(0, snapshot.tip - m.highestComplete) : null;

  return (
    <footer className="sticky bottom-0 z-30 hidden border-t border-hairline bg-panel/95 backdrop-blur-md md:block">
      <dl className="flex h-8 items-center overflow-x-auto text-xs [scrollbar-width:none]">
        <div className="flex shrink-0 items-center gap-1.5 border-r border-hairline px-3">
          <span
            aria-hidden="true"
            className={cn("size-1.5 rounded-full", link === "open" ? "bg-live" : link === "connecting" ? "bg-replay" : "bg-gap")}
          />
          <dt className="sr-only">Server</dt>
          <dd className="text-muted-foreground">
            {link === "open" ? "gapless-server" : link === "connecting" ? "connecting…" : "server offline"}
          </dd>
        </div>
        <Item term="Slot" title="Highest slot fully received">
          {fmt.slot(m?.highestComplete)}
        </Item>
        <Item term="Lag" title="Slots behind the chain tip">
          <span className={cn(lag !== null && lag > 8 && "text-replay")}>{lag === null ? fmt.NONE : `${fmt.int(lag)} slots`}</span>
        </Item>
        <Item term="Verified to" title="Every slot up to here has been checked against RPC">
          <span className="text-verified">{fmt.slot(snapshot?.verifiedThrough)}</span>
        </Item>
        <Item term="Rate">{fmt.rate(m?.txPerSec)}</Item>
        <Item term="Latency" title="Solami stamp to receipt, smoothed">
          {fmt.ms(m?.latencyMs)}
        </Item>
        {solami ? (
          <Item term="Solami" title="Our stream, from Solami's account API">
            {solami.region ?? fmt.NONE} · buffer {fmt.int(solami.bufferPending)}/{fmt.int(solami.bufferSize)} ·{" "}
            {fmt.throughput(solami.throughputBps)}
          </Item>
        ) : null}
        <Item term="Mode">{snapshot?.mode === "offline" ? "offline fixture" : snapshot ? "mainnet" : fmt.NONE}</Item>
        <Item term="Up">{snapshot ? fmt.duration(now - snapshot.startedAt) : fmt.NONE}</Item>
        <Item term="UTC" title="Wall clock">
          {new Date(now).toISOString().slice(11, 19)}
        </Item>
      </dl>
    </footer>
  );
}
