import { Panel } from "@/components/panel";
import { RollingNumber } from "@/components/rolling-number";
import { Skeleton } from "@/components/ui/skeleton";
import { useFeed, type BufferSample } from "@/lib/feed";
import * as fmt from "@/lib/format";
import { cn } from "@/lib/utils";

type Level = "live" | "replay" | "gap";

const fill: Record<Level, string> = { live: "bg-live", replay: "bg-replay", gap: "bg-gap" };
const stroke: Record<Level, string> = { live: "var(--live)", replay: "var(--replay)", gap: "var(--gap)" };

function level(ratio: number): Level {
  return ratio >= 0.85 ? "gap" : ratio >= 0.5 ? "replay" : "live";
}

/** Messages per second the buffer is gaining (or losing) over the last ~20 seconds. */
function trend(samples: BufferSample[]): number | null {
  const last = samples[samples.length - 1];
  if (!last) return null;
  const first = samples.find((s) => last.at - s.at <= 20_000);
  if (!first || last.at - first.at < 4_000) return null;
  return ((last.pending - first.pending) * 1000) / (last.at - first.at);
}

function Sparkline({ samples, size, tone }: { samples: BufferSample[]; size: number; tone: Level }) {
  const W = 300;
  const H = 48;
  if (samples.length < 2) {
    return <div className="h-12 border-b border-hairline" aria-hidden="true" />;
  }
  const t0 = samples[0].at;
  const span = Math.max(1, samples[samples.length - 1].at - t0);
  const points = samples.map((s) => {
    const x = ((s.at - t0) / span) * W;
    const y = H - 1 - Math.min(1, s.pending / size) * (H - 4);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const line = `M${points.join(" L")}`;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" className="block h-12 w-full" aria-hidden="true">
      <line x1="0" x2={W} y1="3" y2="3" stroke="var(--gap)" strokeOpacity="0.5" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
      <path d={`${line} L${W},${H} L0,${H} Z`} fill={stroke[tone]} fillOpacity="0.12" />
      <path d={line} fill="none" stroke={stroke[tone]} strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

/** Solami's send buffer for our stream: how far behind the consumer is, and how long until it's dropped. */
export function BufferGauge() {
  const snapshot = useFeed((s) => s.snapshot);
  const samples = useFeed((s) => s.buffer);
  const solami = snapshot?.solami ?? null;
  const offline = snapshot?.mode === "offline";

  const aside = solami ? (
    solami.emulated ? (
      <span>emulated offline</span>
    ) : (
      <span className="num">{solami.region ?? "region --"}</span>
    )
  ) : null;

  if (!snapshot || !solami) {
    return (
      <Panel title="Solami buffer" aside={aside} bodyClassName="flex flex-col gap-3 p-4">
        <Skeleton className="h-7 w-40" />
        <Skeleton className="h-2 w-full" />
        <p className="text-xs text-muted-foreground">
          {snapshot ? "Waiting for Solami's account API to list our stream." : "Connecting…"}
        </p>
      </Panel>
    );
  }

  const size = solami.bufferSize || 8_192;
  const ratio = Math.min(1, solami.bufferPending / size);
  const tone = level(ratio);
  const rate = trend(samples);
  const streaming = snapshot.state.kind === "live" || snapshot.state.kind === "replaying";

  let status: string;
  if (!streaming) status = "No stream right now.";
  else if (rate !== null && rate > 3) {
    const eta = ((size - solami.bufferPending) / rate) * 1000;
    status = `Filling at +${fmt.int(rate)}/s. Solami drops the stream in about ${fmt.duration(eta)}.`;
  } else if (rate !== null && rate < -3) status = `Draining at ${fmt.int(-rate)}/s.`;
  else if (solami.bufferPending < 64) status = "Empty. The consumer keeps up.";
  else status = "Steady.";

  return (
    <Panel title="Solami buffer" aside={aside} bodyClassName="flex flex-col gap-3 p-4">
      <div className="flex items-baseline justify-between gap-3">
        <p className="flex items-baseline gap-1.5">
          <RollingNumber
            value={fmt.int(solami.bufferPending)}
            className={cn("text-2xl font-medium tracking-tight", tone !== "live" && (tone === "gap" ? "text-gap" : "text-replay"))}
          />
          <span className="num text-sm text-faint">/ {fmt.int(size)}</span>
          <span className="text-xs text-muted-foreground">messages pending</span>
        </p>
        <span className="num text-sm text-muted-foreground">{fmt.percent(ratio)}</span>
      </div>

      <div
        role="meter"
        aria-label="Solami buffer pending"
        aria-valuemin={0}
        aria-valuemax={size}
        aria-valuenow={solami.bufferPending}
        aria-valuetext={`${fmt.int(solami.bufferPending)} of ${fmt.int(size)} messages`}
        className="relative h-2 overflow-hidden rounded-full bg-muted"
      >
        <div
          className={cn("absolute inset-0 origin-left rounded-full transition-transform duration-700 ease-crisp", fill[tone])}
          style={{ transform: `scaleX(${Math.max(ratio, 0.004)})` }}
        />
        {[0.25, 0.5, 0.75].map((t) => (
          <span key={t} aria-hidden="true" className="absolute inset-y-0 w-px bg-background/70" style={{ left: `${t * 100}%` }} />
        ))}
      </div>

      <Sparkline samples={samples} size={size} tone={tone} />

      <p role="status" className="text-xs text-pretty text-muted-foreground">
        {status}
      </p>

      <dl className="grid grid-cols-3 gap-3 border-t border-hairline pt-3 text-xs">
        {offline ? (
          <div className="col-span-3 text-muted-foreground">
            The fixture source emulates Solami's default buffer and closes a stream that falls{" "}
            <span className="num">{fmt.int(size)}</span> messages behind, as Solami does.
          </div>
        ) : (
          <>
            <div className="min-w-0">
              <dt className="text-faint">Throughput</dt>
              <dd className="num mt-0.5 truncate">{fmt.throughput(solami.throughputBps)}</dd>
            </div>
            <div className="min-w-0">
              <dt className="text-faint">Streamed</dt>
              <dd className="num mt-0.5 truncate">{fmt.bytes(solami.bytesStreamed)}</dd>
            </div>
            <div className="min-w-0">
              <dt className="text-faint">Streams open</dt>
              <dd className="num mt-0.5 truncate">{fmt.int(solami.liveStreams)}</dd>
            </div>
          </>
        )}
      </dl>
    </Panel>
  );
}
