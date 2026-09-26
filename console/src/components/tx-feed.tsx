import { useVirtualizer } from "@tanstack/react-virtual";
import { Pause } from "lucide-react";
import { useRef, useState } from "react";

import { Panel } from "@/components/panel";
import { RowsSkeleton } from "@/components/feed-states";
import { Segmented } from "@/components/segmented";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import type { Tx } from "@/lib/types";
import { cn } from "@/lib/utils";

const ROW = 36;

type Filter = "all" | "recovered" | "duplicate";

const originLabel: Record<Tx["origin"], string> = {
  live: "Live",
  replay: "Replayed",
  patch: "Patched",
  duplicate: "Duplicate",
};

const originTitle: Record<Tx["origin"], string> = {
  live: "Delivered live",
  replay: "Recovered by replay after an outage",
  patch: "Recovered by the handoff patch: Solami dropped it at the switch to live",
  duplicate: "Sent again by Solami after a resume; Gapless dropped it",
};

const kindLabel: Record<Tx["kind"], string> = {
  buy: "Buy",
  sell: "Sell",
  create: "Launch",
  complete: "Graduated",
  other: "Other",
};

function OriginMark({ origin }: { origin: Tx["origin"] }) {
  return (
    <span className="flex items-center gap-2 text-xs" title={originTitle[origin]}>
      <span
        aria-hidden="true"
        className={cn(
          "size-1.5 shrink-0 rounded-full",
          origin === "live" && "bg-live",
          origin === "replay" && "bg-replay",
          origin === "patch" && "bg-replay ring-2 ring-replay/30",
          origin === "duplicate" && "border border-faint",
        )}
      />
      <span className={cn(origin === "live" ? "text-muted-foreground" : origin === "duplicate" ? "text-faint" : "text-replay")}>
        {originLabel[origin]}
      </span>
    </span>
  );
}

function Row({ tx }: { tx: Tx }) {
  const dup = tx.origin === "duplicate";
  return (
    <a
      href={`https://solscan.io/tx/${tx.sig}`}
      target="_blank"
      rel="noreferrer"
      className="grid h-9 grid-cols-[5.5rem_4.5rem_minmax(0,1fr)_auto] items-center gap-3 px-4 text-sm transition-colors duration-100 hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset sm:grid-cols-[5.5rem_4.5rem_minmax(0,1fr)_auto_7rem]"
    >
      <OriginMark origin={tx.origin} />
      <span className={cn("text-xs", tx.kind === "other" || dup ? "text-faint" : "text-muted-foreground")}>
        {dup ? "Dropped" : kindLabel[tx.kind]}
      </span>
      <span className="min-w-0 truncate">
        {dup ? (
          <span className="num text-faint line-through decoration-faint/60" title={tx.sig}>
            {fmt.short(tx.sig, 6)}
          </span>
        ) : tx.symbol ? (
          <span className="font-medium">{tx.symbol}</span>
        ) : tx.mint ? (
          <span className="num text-muted-foreground" title={tx.mint}>
            {fmt.short(tx.mint)}
          </span>
        ) : (
          <span className="num text-faint" title={tx.sig}>
            {fmt.short(tx.sig, 6)}
          </span>
        )}
      </span>
      <span className="num text-right text-xs text-muted-foreground">
        {tx.sol !== null ? `${fmt.sol(tx.sol)} SOL` : dup ? "already delivered" : ""}
      </span>
      <span className="num hidden text-right text-xs text-faint sm:block">{fmt.slot(tx.slot)}</span>
    </a>
  );
}

/** The stream's transactions, newest first: a sample, with every replayed and dropped one marked. */
export function TransactionsPanel() {
  const all = useFeed((s) => s.txs);
  const recovered = useFeed((s) => s.recovered);
  const duplicates = useFeed((s) => s.duplicates);
  const ready = useFeed((s) => s.snapshot !== null);
  const [filter, setFilter] = useState<Filter>("all");
  const [frozen, setFrozen] = useState<Tx[] | null>(null);
  const parentRef = useRef<HTMLDivElement>(null);

  const live = filter === "all" ? all : filter === "recovered" ? recovered : duplicates;
  // Hovering or focusing the list freezes it, so rows hold still long enough to read and click.
  const rows = frozen ?? live;
  const fresh = frozen && frozen.length ? Math.max(0, live.indexOf(frozen[0])) : 0;

  // The console doesn't use the React Compiler, so the virtualizer's unmemoizable API is fine.
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW,
    overscan: 10,
    getItemKey: (i) => `${rows[i].origin}:${rows[i].sig}:${rows[i].at}`,
  });

  const pause = () => setFrozen((f) => f ?? live);
  const resume = () => setFrozen(null);
  const choose = (next: Filter) => {
    setFilter(next);
    setFrozen(null);
  };

  return (
    <Panel
      title="Transactions"
      aside={
        frozen ? (
          <span className="flex items-center gap-1.5 text-foreground">
            <Pause aria-hidden="true" className="size-3" />
            Paused{fresh > 0 && <span className="num text-muted-foreground">· {fmt.int(fresh)} new</span>}
          </span>
        ) : (
          <span className="hidden sm:inline">newest first · hover to pause</span>
        )
      }
    >
      <div className="flex items-center justify-between gap-3 border-b border-hairline px-4 py-2">
        <Segmented
          label="Show"
          value={filter}
          onChange={choose}
          options={[
            { value: "all", label: "All" },
            { value: "recovered", label: `Replayed ${recovered.length ? fmt.compact(recovered.length) : ""}`.trim() },
            { value: "duplicate", label: `Duplicates ${duplicates.length ? fmt.compact(duplicates.length) : ""}`.trim() },
          ]}
        />
        <span className="hidden text-xs text-muted-foreground sm:inline">
          <span className="num">{fmt.int(rows.length)}</span> shown
        </span>
      </div>
      {!ready ? (
        <RowsSkeleton />
      ) : (
        <div
          ref={parentRef}
          onPointerEnter={pause}
          onPointerLeave={resume}
          onFocus={pause}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) resume();
          }}
          className="h-[26rem] overflow-y-auto overscroll-contain"
        >
          {rows.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">
              {filter === "all"
                ? "No transactions yet. They appear as the stream delivers them."
                : filter === "recovered"
                  ? "Nothing replayed recently. Break the stream and replayed transactions show up here, marked in amber."
                  : "No duplicates recently. After a resume, anything Solami sends twice is dropped and listed here."}
            </p>
          ) : (
            <div role="list" aria-label="Transactions" className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
              {virtualizer.getVirtualItems().map((item) => (
                <div
                  key={item.key}
                  role="listitem"
                  className="absolute inset-x-0 top-0 border-b border-hairline"
                  style={{ height: ROW, transform: `translateY(${item.start}px)` }}
                >
                  <Row tx={rows[item.index]} />
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </Panel>
  );
}
