// Display formatting for every number in the console. Rules (number-formatting spec):
// null / NaN / Infinity render as "--"; never scientific notation; never "-0"; compact
// abbreviations (K/M/B) only for counts and bytes; render with the `.num` class (mono,
// tabular) so live values don't jitter. Callers keep raw values for copy/export.

export const NONE = "--";

const valid = (n: number | null | undefined): n is number => typeof n === "number" && Number.isFinite(n);

const group = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** Exact integer with grouping: 11,015. */
export function int(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  const v = Math.round(n);
  return group.format(v === 0 ? 0 : v);
}

/** Slot numbers are identifiers: always exact, grouped for scanning. */
export function slot(n: number | null | undefined): string {
  return int(n);
}

/** Compact count: 987, 12.3K, 4.56M. */
export function compact(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  const abs = Math.abs(n);
  const units: [number, string][] = [
    [1e12, "T"],
    [1e9, "B"],
    [1e6, "M"],
    [1e3, "K"],
  ];
  for (const [size, unit] of units) {
    if (abs >= size) {
      const v = n / size;
      const digits = Math.abs(v) >= 100 ? 0 : Math.abs(v) >= 10 ? 1 : 2;
      return `${trimZeros(v.toFixed(digits))}${unit}`;
    }
  }
  return int(n);
}

/** A rate per second: 71/s, 2.3K/s. */
export function rate(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  if (n > 0 && n < 1) return `${trimZeros(n.toFixed(1))}/s`;
  return `${compact(n)}/s`;
}

/** SOL volume. The console has no SOL price, so decimals scale with magnitude. */
export function sol(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  if (n === 0) return "0";
  const abs = Math.abs(n);
  if (abs < 0.0001) return "<0.0001";
  if (abs >= 10_000) return compact(n);
  const digits = abs >= 100 ? 1 : abs >= 1 ? 2 : 4;
  const text = n.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });
  return noNegativeZero(text);
}

/** Milliseconds: 46ms, 1.2s, 80s. */
export function ms(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  if (n < 1000) return `${Math.round(n)}ms`;
  return duration(n);
}

/** A duration from milliseconds: 1.2s, 80s, 3m 20s, 1h 04m. */
export function duration(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  const s = n / 1000;
  if (s < 10) return `${trimZeros(s.toFixed(1))}s`;
  if (s < 90) return `${Math.round(s)}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(Math.round(s % 60)).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/** Bytes: 930 B, 21.2 MB. */
export function bytes(n: number | null | undefined): string {
  if (!valid(n)) return NONE;
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (Math.abs(v) >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const digits = i === 0 || Math.abs(v) >= 100 ? 0 : 1;
  return `${trimZeros(v.toFixed(digits))} ${units[i]}`;
}

/** Throughput from bytes per second: 482 KB/s. */
export function throughput(bps: number | null | undefined): string {
  if (!valid(bps)) return NONE;
  return `${bytes(bps)}/s`;
}

/** Percent from a 0..1 fraction: 100%, 99.96%, <0.01%. */
export function percent(fraction: number | null | undefined): string {
  if (!valid(fraction)) return NONE;
  const p = fraction * 100;
  if (p === 0) return "0%";
  if (p > 0 && p < 0.01) return "<0.01%";
  if (p === 100) return "100%";
  return `${noNegativeZero(trimZeros(p.toFixed(p >= 99 && p < 100 ? 2 : 1)))}%`;
}

/** Time of day in 24h: 14:02:07. */
export function clock(ms: number | null | undefined): string {
  if (!valid(ms)) return NONE;
  return new Date(ms).toLocaleTimeString("en-GB", { hour12: false });
}

/** Relative time: 4s ago, 3m ago. */
export function ago(ms: number | null | undefined, now = Date.now()): string {
  if (!valid(ms)) return NONE;
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

/** Truncated base58 for scanning; always pair with the full value (title / copy). */
export function short(id: string | null | undefined, keep = 4): string {
  if (!id) return NONE;
  if (id.length <= keep * 2 + 1) return id;
  return `${id.slice(0, keep)}…${id.slice(-keep)}`;
}

function trimZeros(s: string): string {
  return s.includes(".") ? s.replace(/\.?0+$/, "") : s;
}

function noNegativeZero(s: string): string {
  return /^-0(\.0*)?$/.test(s) ? s.slice(1) : s;
}

/** Capitalise the first letter: server reasons arrive lowercase ("cut by the client"). */
export function sentence(s: string | null | undefined): string {
  if (!s) return "";
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** "1 step", "3 steps". */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${int(n)} ${n === 1 ? one : many}`;
}
