import { useFeed } from "@/lib/feed";
import { streamStatus, toneBg, toneText } from "@/lib/status";
import { cn } from "@/lib/utils";

/** The stream's state as a quiet pill: dot + word. The dot breathes only while live. */
export function StreamPill({ className }: { className?: string }) {
  const link = useFeed((s) => s.link);
  const snapshot = useFeed((s) => s.snapshot);
  const status = streamStatus(link, snapshot);
  return (
    <span
      role="status"
      aria-live="polite"
      title={status.detail}
      className={cn(
        "inline-flex h-7 items-center gap-2 rounded-full border border-hairline bg-panel px-2.5 text-xs font-medium",
        toneText[status.tone],
        className,
      )}
    >
      <span className="relative flex size-2" aria-hidden="true">
        {status.tone === "live" && (
          <span className="absolute inset-0 rounded-full bg-live opacity-60 motion-safe:animate-ping [animation-duration:2.4s]" />
        )}
        <span className={cn("relative size-2 rounded-full", toneBg[status.tone])} />
      </span>
      {status.label}
      <span className="sr-only">: {status.detail}</span>
    </span>
  );
}
