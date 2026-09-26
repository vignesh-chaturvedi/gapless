import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import type { Tx } from "@/lib/types";
import { cn } from "@/lib/utils";

const originDot: Record<Tx["origin"], string> = {
  live: "bg-live",
  replay: "bg-replay",
  patch: "bg-replay ring-2 ring-replay/30",
};

const originLabel: Record<Tx["origin"], string> = {
  live: "Live",
  replay: "Replayed",
  patch: "Recovered by the handoff patch",
};

const kindLabel: Record<Tx["kind"], string> = {
  buy: "Buy",
  sell: "Sell",
  create: "Launch",
  complete: "Graduated",
  other: "Other",
};

/** Recent transactions, newest first. A sample of the stream, not every transaction. */
export function TxFeed({ limit = 40 }: { limit?: number }) {
  const txs = useFeed((s) => s.txs);
  const rows = txs.slice(0, limit);
  if (rows.length === 0) {
    return <p className="p-4 text-sm text-muted-foreground">No transactions yet. They appear as the stream delivers them.</p>;
  }
  return (
    <ol className="divide-y divide-hairline">
      {rows.map((tx) => (
        <li key={tx.sig} className="grid grid-cols-[auto_4.5rem_1fr_auto] items-center gap-3 px-4 py-2 text-sm">
          <span
            title={originLabel[tx.origin]}
            aria-label={originLabel[tx.origin]}
            className={cn("size-1.5 rounded-full", originDot[tx.origin])}
          />
          <span className={cn("text-xs", tx.kind === "other" ? "text-faint" : "text-muted-foreground")}>
            {kindLabel[tx.kind]}
          </span>
          <span className="min-w-0 truncate">
            {tx.symbol ? (
              <span className="font-medium">{tx.symbol}</span>
            ) : tx.mint ? (
              <span className="num text-muted-foreground" title={tx.mint}>
                {fmt.short(tx.mint)}
              </span>
            ) : (
              <span className="text-faint">No Pump.fun event</span>
            )}
          </span>
          <span className="num text-right text-xs text-muted-foreground">
            {tx.sol !== null ? (
              `${fmt.sol(tx.sol)} SOL`
            ) : (
              <a
                href={`https://solscan.io/tx/${tx.sig}`}
                target="_blank"
                rel="noreferrer"
                title={tx.sig}
                className="rounded-sm underline-offset-2 hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                {fmt.short(tx.sig)}
              </a>
            )}
          </span>
        </li>
      ))}
    </ol>
  );
}
