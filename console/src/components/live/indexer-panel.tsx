import { Panel } from "@/components/panel";
import { RollingNumber } from "@/components/rolling-number";
import { Skeleton } from "@/components/ui/skeleton";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import type { Indexer, Minute } from "@/lib/types";
import { useNow } from "@/lib/use-now";

function Stat({ label, value, unit }: { label: string; value: string; unit?: string }) {
  return (
    <div className="min-w-0">
      <dt className="label">{label}</dt>
      <dd className="mt-1 flex items-baseline gap-1">
        <RollingNumber value={value} className="text-lg font-medium tracking-tight" />
        {unit && <span className="num text-xs text-muted-foreground">{unit}</span>}
      </dd>
    </div>
  );
}

/** Transactions per minute, with the part recovered by replay hatched in amber. */
function MinuteChart({ minutes }: { minutes: Minute[] }) {
  const W = 300;
  const H = 72;
  const slots = 30;
  const bars = minutes.slice(-slots);
  if (bars.length === 0) return <div className="h-[4.5rem]" aria-hidden="true" />;
  const peak = Math.max(1, ...bars.map((m) => m.txs));
  const pitch = W / slots;
  const offset = (slots - bars.length) * pitch;
  const replayed = bars.reduce((sum, m) => sum + m.replayed, 0);
  return (
    <figure className="flex flex-col gap-1.5">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        className="block h-[4.5rem] w-full"
        role="img"
        aria-label={`Transactions per minute over the last ${fmt.plural(bars.length, "minute")}; ${fmt.int(replayed)} recovered by replay.`}
      >
        <defs>
          <pattern id="replay-hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect width="4" height="4" fill="var(--replay)" opacity="0.35" />
            <line x1="0" y1="0" x2="0" y2="4" stroke="var(--replay)" strokeWidth="1.5" />
          </pattern>
        </defs>
        {bars.map((m, i) => {
          const x = offset + i * pitch + 1;
          const w = pitch - 2;
          const total = (m.txs / peak) * (H - 2);
          const rec = (m.replayed / peak) * (H - 2);
          const current = i === bars.length - 1;
          return (
            <g key={m.minute} opacity={current ? 0.55 : 1}>
              <title>{`${fmt.clock(m.minute * 60_000).slice(0, 5)}: ${fmt.plural(m.txs, "transaction")}, ${fmt.int(m.replayed)} replayed, ${fmt.plural(m.trades, "trade")}`}</title>
              <rect x={x} y={H - total} width={w} height={total - rec} fill="var(--foreground)" opacity="0.28" />
              {rec > 0 && <rect x={x} y={H - rec} width={w} height={rec} fill="url(#replay-hatch)" />}
            </g>
          );
        })}
      </svg>
      <figcaption className="flex justify-between text-xs text-muted-foreground">
        <span className="num">{fmt.clock(bars[0].minute * 60_000).slice(0, 5)}</span>
        <span>per minute, by on-chain time</span>
        <span className="num">{fmt.clock(bars[bars.length - 1].minute * 60_000).slice(0, 5)}</span>
      </figcaption>
    </figure>
  );
}

function Pressure({ indexer }: { indexer: Indexer }) {
  const total = indexer.buySol + indexer.sellSol;
  const buy = total > 0 ? indexer.buySol / total : 0.5;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between text-xs">
        <span>
          Buys <span className="num text-muted-foreground">{fmt.percent(buy)}</span>
        </span>
        <span className="text-muted-foreground">by SOL volume</span>
        <span>
          Sells <span className="num text-muted-foreground">{fmt.percent(1 - buy)}</span>
        </span>
      </div>
      <div className="flex h-1.5 gap-0.5 overflow-hidden rounded-full" aria-hidden="true">
        <span className="rounded-l-full bg-foreground/70 transition-[flex-grow] duration-700 ease-crisp" style={{ flexGrow: buy }} />
        <span className="rounded-r-full bg-foreground/20 transition-[flex-grow] duration-700 ease-crisp" style={{ flexGrow: 1 - buy }} />
      </div>
    </div>
  );
}

/** A tiny Pump.fun indexer fed by the stream. Its counts stay exact through every incident. */
export function IndexerPanel() {
  const indexer = useFeed((s) => s.snapshot?.indexer);
  const duplicates = useFeed((s) => s.snapshot?.metrics.duplicates ?? 0);
  const now = useNow(5_000);

  return (
    <Panel title="Pump.fun · indexed from the stream" aside={<span className="hidden sm:inline">each transaction counted once</span>} bodyClassName="flex flex-col gap-4 p-4">
      {!indexer ? (
        <div className="flex flex-col gap-3" aria-hidden="true">
          <Skeleton className="h-12" />
          <Skeleton className="h-[4.5rem]" />
        </div>
      ) : (
        <>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-3 min-[480px]:grid-cols-3 sm:grid-cols-5">
            <Stat label="Trades" value={fmt.int(indexer.trades)} />
            <Stat label="Bought" value={fmt.sol(indexer.buySol)} unit="SOL" />
            <Stat label="Sold" value={fmt.sol(indexer.sellSol)} unit="SOL" />
            <Stat label="Traders" value={fmt.int(indexer.uniqueTraders)} />
            <Stat label="Launches" value={fmt.int(indexer.launches)} />
          </dl>
          <Pressure indexer={indexer} />
          <MinuteChart minutes={indexer.minutes} />
          <p className="text-xs text-pretty text-muted-foreground">
            {indexer.replayed > 0 ? (
              <>
                <span className="text-replay">Hatched</span>: <span className="num">{fmt.int(indexer.replayed)}</span>{" "}
                transactions recovered by replay, counted in the minute they happened. Without Gapless those minutes come
                up short; without dedup, <span className="num">{fmt.int(duplicates)}</span> would have counted twice.
              </>
            ) : (
              "Trades count by their on-chain timestamp, so anything replayed after an outage lands in the minute it happened."
            )}
          </p>
          {indexer.recentLaunches.length > 0 && (
            <div className="border-t border-hairline pt-3">
              <h3 className="label">Latest launches</h3>
              <ul className="mt-2 flex flex-col gap-1.5">
                {indexer.recentLaunches.slice(0, 4).map((launch) => (
                  <li key={launch.mint} className="grid grid-cols-[minmax(0,6rem)_1fr_auto] items-baseline gap-3 text-sm">
                    <a
                      href={`https://solscan.io/token/${launch.mint}`}
                      target="_blank"
                      rel="noreferrer"
                      title={launch.mint}
                      className="truncate rounded-sm font-medium underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                    >
                      {launch.symbol || fmt.short(launch.mint)}
                    </a>
                    <span className="truncate text-muted-foreground">{launch.name}</span>
                    <span className="num text-xs text-faint">{fmt.ago(launch.at, now)}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </>
      )}
    </Panel>
  );
}
