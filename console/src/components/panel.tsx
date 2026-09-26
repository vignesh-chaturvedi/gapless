import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

interface PanelProps {
  /** Instrument label: short, rendered uppercase. */
  title: string;
  /** Right side of the header: a status, a count, an action. */
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
  id?: string;
}

/** A flat panel with a hairline border and an instrument label. Panels don't nest. */
export function Panel({ title, aside, children, className, bodyClassName, id }: PanelProps) {
  const headingId = id ? `${id}-title` : undefined;
  return (
    <section
      id={id}
      aria-labelledby={headingId}
      className={cn("flex min-w-0 flex-col rounded-lg border border-hairline bg-panel", className)}
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-3 border-b border-hairline px-4">
        <h2 id={headingId} className="label">
          {title}
        </h2>
        {aside && <div className="flex min-w-0 items-center gap-2 text-xs text-muted-foreground">{aside}</div>}
      </header>
      <div className={cn("min-h-0 flex-1", bodyClassName)}>{children}</div>
    </section>
  );
}

interface ReadoutProps {
  label: string;
  value: ReactNode;
  unit?: string;
  hint?: ReactNode;
  tone?: "default" | "live" | "replay" | "gap" | "verified";
  className?: string;
}

const readoutTone = {
  default: "text-foreground",
  live: "text-live",
  replay: "text-replay",
  gap: "text-gap",
  verified: "text-verified",
};

/** One instrument readout: label over a mono value, with an optional unit and hint. */
export function Readout({ label, value, unit, hint, tone = "default", className }: ReadoutProps) {
  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <span className="label">{label}</span>
      <span className="flex items-baseline gap-1">
        <span className={cn("num text-xl font-medium tracking-tight", readoutTone[tone])}>{value}</span>
        {unit && <span className="num text-xs text-muted-foreground">{unit}</span>}
      </span>
      {hint && <span className="truncate text-xs text-muted-foreground">{hint}</span>}
    </div>
  );
}
