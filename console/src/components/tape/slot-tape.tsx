import { useEffect, useRef, useState } from "react";

import * as fmt from "@/lib/format";
import { liveTape, type TapeSource } from "@/lib/tape";
import { useTheme } from "@/lib/theme";
import type { SlotCell } from "@/lib/types";
import { cn } from "@/lib/utils";

const PITCH = 8; // px per slot
const BAR = 5; // bar width
const PAD_X = 12;

interface Palette {
  live: string;
  replay: string;
  gap: string;
  verified: string;
  faint: string;
  hairline: string;
}

function readPalette(): Palette {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim();
  return {
    live: v("--live"),
    replay: v("--replay"),
    gap: v("--gap"),
    verified: v("--verified"),
    faint: v("--faint"),
    hairline: v("--border"),
  };
}

function hatch(ctx: CanvasRenderingContext2D, color: string, dpr: number): CanvasPattern | string {
  const size = Math.round(4 * dpr);
  const tile = document.createElement("canvas");
  tile.width = size;
  tile.height = size;
  const t = tile.getContext("2d");
  if (!t) return color;
  t.fillStyle = color;
  t.globalAlpha = 0.35;
  t.fillRect(0, 0, size, size);
  t.globalAlpha = 1;
  t.strokeStyle = color;
  t.lineWidth = 1.25 * dpr;
  t.beginPath();
  t.moveTo(-1, size + 1);
  t.lineTo(size + 1, -1);
  t.stroke();
  return ctx.createPattern(tile, "repeat") ?? color;
}

interface SlotTapeProps {
  source?: TapeSource;
  height?: number;
  className?: string;
  /** Accessible summary; updated by the parent at a calm cadence. */
  label: string;
  /** Hovering shows a slot inspector; clicking a replayed slot calls this with its incident. */
  onSelectIncident?: (incident: number) => void;
}

/** The slot under the pointer, for the inspector. */
interface Hover {
  slot: number;
  x: number;
  cell: SlotCell | null;
  /** What an empty slot means at that position. */
  empty: "skipped" | "in-flight" | "recovering" | "gap" | null;
}

function Inspector({ hover, width }: { hover: Hover; width: number }) {
  const { cell } = hover;
  let state: string;
  if (cell) {
    const origin = cell.origin === "replay" ? "Replayed" : "Live";
    state =
      cell.verified === "missing"
        ? `${origin} · verified, ${fmt.int(cell.missing)} missing`
        : cell.verified === "repaired"
          ? `${origin} · verified, repaired from RPC`
          : cell.verified === "ok"
            ? `${origin} · verified complete`
            : `${origin} · not verified yet`;
  } else {
    state =
      hover.empty === "skipped"
        ? "No block: skipped by its leader"
        : hover.empty === "in-flight"
          ? "Arriving"
          : hover.empty === "recovering"
            ? "Missed, being replayed"
            : "Missed while disconnected";
  }
  const left = Math.min(Math.max(hover.x - 96, 4), Math.max(4, width - 196));
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute top-2 z-10 w-48 rounded-md border border-hairline bg-popover/95 px-3 py-2 text-xs shadow-sm backdrop-blur-sm"
      style={{ left }}
    >
      <p className="num font-medium text-foreground">Slot {fmt.slot(hover.slot)}</p>
      <p className="mt-0.5 text-muted-foreground">{state}</p>
      {cell && (
        <p className="mt-0.5 text-muted-foreground">
          <span className="num text-foreground">{fmt.int(cell.txs)}</span> transaction{cell.txs === 1 ? "" : "s"}
        </p>
      )}
      {cell?.incident != null && cell.incident > 0 && (
        <p className="mt-1 text-replay">
          Incident <span className="num">#{cell.incident}</span> · click for details
        </p>
      )}
    </div>
  );
}

export function SlotTape({ source = liveTape, height = 96, className, label, onSelectIncident }: SlotTapeProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const theme = useTheme();
  const pointer = useRef<number | null>(null);
  const [hover, setHover] = useState<Hover | null>(null);
  const [width, setWidth] = useState(0);
  const interactive = Boolean(onSelectIncident);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    let palette = readPalette();
    let dpr = window.devicePixelRatio || 1;
    let width = 0;
    let replayFill: CanvasPattern | string = palette.replay;
    let scale = 40; // tx count that fills a bar; adapts to traffic
    let shownTip: number | null = null;
    let frame = 0;
    let hovered: string | null = null;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      dpr = window.devicePixelRatio || 1;
      width = rect.width;
      setWidth(rect.width);
      canvas.width = Math.round(rect.width * dpr);
      canvas.height = Math.round(height * dpr);
      palette = readPalette();
      replayFill = hatch(ctx, palette.replay, dpr);
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);

    const draw = () => {
      frame = requestAnimationFrame(draw);
      const f = source();
      const target = Math.max(f.tip ?? 0, f.highestComplete ?? 0);
      if (!target) {
        ctx.clearRect(0, 0, canvas.width, canvas.height);
        return;
      }
      // Glide toward the tip between one-second ticks; jump when motion is reduced or far off.
      if (shownTip === null || reduced.matches || Math.abs(target - shownTip) > 40) shownTip = target;
      else shownTip += (target - shownTip) * 0.08;

      const count = Math.ceil((width - PAD_X * 2) / PITCH) + 2;
      const newest = Math.floor(shownTip);
      const offset = (shownTip - newest) * PITCH;
      let first: number | null = null;
      let peak = 0;
      for (const slot of f.tape.keys()) {
        if (first === null || slot < first) first = slot;
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);
      const floor = height - 14;
      const top = interactive ? 20 : 10;
      const usable = floor - top;
      // The stretch of slots each incident replayed or patched, for the brackets above the bars.
      const runs = new Map<number, { from: number; to: number }>();
      let next: Hover | null = null;
      const px = pointer.current;

      // Baseline.
      ctx.fillStyle = palette.hairline;
      ctx.fillRect(PAD_X, floor + 0.5, width - PAD_X * 2, 1);

      for (let i = 0; i < count; i++) {
        const slot = newest - i;
        const x = width - PAD_X - BAR - i * PITCH + offset;
        if (x < PAD_X - BAR) break;
        if (first !== null && slot < first) break;
        const cell = f.tape.get(slot);
        const high = f.highestComplete ?? slot;
        if (px !== null && Math.abs(px - (x + BAR / 2)) <= PITCH / 2) {
          next = {
            slot,
            x: x + BAR / 2,
            cell: cell?.complete ? cell : null,
            empty: cell?.complete
              ? null
              : slot <= high
                ? "skipped"
                : f.stream === "live"
                  ? "in-flight"
                  : f.stream === "replaying"
                    ? "recovering"
                    : "gap",
          };
        }
        if (interactive && cell?.incident != null && cell.incident > 0) {
          const run = runs.get(cell.incident);
          if (run) run.from = x;
          else runs.set(cell.incident, { from: x, to: x + BAR });
        }

        if (cell?.complete) {
          peak = Math.max(peak, cell.txs);
          const h = Math.max(3, Math.min(usable, usable * Math.sqrt(cell.txs / scale)));
          const y = floor - h;
          if (cell.verified === "missing") {
            ctx.fillStyle = palette.gap;
            ctx.fillRect(x, y, BAR, h);
          } else if (cell.verified) {
            ctx.fillStyle = palette.verified;
            ctx.fillRect(x, y, BAR, h);
            // Cap: the "checked" cue. Red when Solami dropped some and RPC filled them in.
            if (cell.verified === "repaired") ctx.fillStyle = palette.gap;
            ctx.fillRect(x - 1, y - 3, BAR + 2, cell.verified === "repaired" ? 2 : 1.5);
          } else if (cell.origin === "replay") {
            ctx.fillStyle = replayFill;
            ctx.fillRect(x, y, BAR, h);
            ctx.fillStyle = palette.replay;
            ctx.fillRect(x, y, BAR, 1.5);
          } else {
            ctx.fillStyle = palette.live;
            ctx.fillRect(x, y, BAR, h);
          }
          continue;
        }

        // At or below the highest complete slot, a slot with no cell was skipped by its leader
        // (no block): normal, and not a gap.
        if (slot <= high) {
          ctx.fillStyle = palette.faint;
          ctx.globalAlpha = 0.6;
          ctx.fillRect(x + BAR / 2 - 1, floor - 2, 2, 2);
          ctx.globalAlpha = 1;
          continue;
        }
        // Past it: in flight while streaming, being recovered while replaying, a gap while down.
        if (f.stream === "live") {
          ctx.strokeStyle = palette.faint;
          ctx.globalAlpha = 0.5;
          ctx.strokeRect(x + 0.5, floor - 7.5, BAR - 1, 7);
          ctx.globalAlpha = 1;
          continue;
        }
        ctx.strokeStyle = f.stream === "replaying" ? palette.replay : palette.gap;
        ctx.lineWidth = 1;
        ctx.strokeRect(x + 0.5, top + usable * 0.45 + 0.5, BAR - 1, usable * 0.55 - 1);
      }

      // Verified-through marker.
      if (f.verifiedThrough !== null) {
        const i = newest - f.verifiedThrough;
        const x = width - PAD_X - BAR - i * PITCH + offset + BAR + 1.5;
        if (x > PAD_X && x < width - PAD_X) {
          ctx.fillStyle = palette.verified;
          ctx.globalAlpha = 0.55;
          for (let y = 2; y < floor; y += 4) ctx.fillRect(x, y, 1, 2);
          ctx.globalAlpha = 1;
        }
      }

      // Incident brackets: which replayed stretch belongs to which outage.
      for (const [incident, run] of runs) {
        const w = run.to - run.from;
        ctx.fillStyle = palette.replay;
        ctx.globalAlpha = 0.7;
        ctx.fillRect(run.from, 8, w, 1);
        ctx.fillRect(run.from, 8, 1, 4);
        ctx.fillRect(run.to - 1, 8, 1, 4);
        ctx.globalAlpha = 1;
        if (w > 28) {
          const text = `#${incident}`;
          ctx.font = "500 10px 'Geist Mono Variable', ui-monospace, monospace";
          const tw = ctx.measureText(text).width;
          const cx = run.from + w / 2;
          ctx.clearRect(cx - tw / 2 - 3, 2, tw + 6, 12);
          ctx.fillStyle = palette.replay;
          ctx.textBaseline = "middle";
          ctx.fillText(text, cx - tw / 2, 8.5);
        }
      }

      // The inspector's cursor.
      if (next) {
        ctx.fillStyle = palette.faint;
        ctx.globalAlpha = 0.5;
        ctx.fillRect(Math.round(next.x) - 0.5, top - 4, 1, floor - top + 4);
        ctx.globalAlpha = 1;
      }
      const key = next ? `${next.slot}:${next.cell?.verified}:${next.cell?.txs}:${next.empty}:${Math.round(next.x)}` : null;
      if (key !== hovered) {
        hovered = key;
        setHover(next);
      }

      // Let the bar scale follow traffic, slowly.
      if (peak > 0) scale += (Math.max(12, peak * 0.9) - scale) * 0.02;
    };
    frame = requestAnimationFrame(draw);

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [source, height, theme, interactive]);

  if (!interactive) {
    return (
      <canvas ref={canvasRef} role="img" aria-label={label} className={cn("block w-full", className)} style={{ height }} />
    );
  }
  const incident = hover?.cell?.incident;
  return (
    <div className={cn("relative", className)}>
      <canvas
        ref={canvasRef}
        role="img"
        aria-label={label}
        className={cn("block w-full", incident ? "cursor-pointer" : "cursor-crosshair")}
        style={{ height }}
        onPointerMove={(event) => {
          pointer.current = event.clientX - event.currentTarget.getBoundingClientRect().left;
        }}
        onPointerLeave={() => {
          pointer.current = null;
        }}
        onClick={() => {
          if (incident != null && incident > 0) onSelectIncident?.(incident);
        }}
      />
      {hover && <Inspector hover={hover} width={width} />}
    </div>
  );
}

/** The four states, spelled out. Shapes match the tape. */
export function TapeLegend({ className }: { className?: string }) {
  const items = [
    { label: "Live", swatch: <span className="h-3 w-1.5 rounded-[1px] bg-live" /> },
    {
      label: "Replayed",
      swatch: (
        <span
          className="h-3 w-1.5 rounded-[1px] border-t border-replay"
          style={{
            background:
              "repeating-linear-gradient(135deg, var(--replay) 0 1px, color-mix(in oklch, var(--replay) 35%, transparent) 1px 3px)",
          }}
        />
      ),
    },
    { label: "Gap", swatch: <span className="h-3 w-1.5 rounded-[1px] border border-gap" /> },
    {
      label: "Verified",
      swatch: (
        <span className="relative h-3 w-1.5 rounded-[1px] bg-verified after:absolute after:-top-1 after:-left-px after:h-px after:w-2 after:bg-verified" />
      ),
    },
  ];
  return (
    <ul className={cn("flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground", className)}>
      {items.map((item) => (
        <li key={item.label} className="flex items-center gap-1.5">
          <span aria-hidden="true" className="flex h-3 items-end">
            {item.swatch}
          </span>
          {item.label}
        </li>
      ))}
    </ul>
  );
}
