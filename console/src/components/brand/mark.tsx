import { cn } from "@/lib/utils";

/** Four tape cells: verified, replayed, live, in flight. The whole story in 22px. */
export function Mark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 22 16" className={cn("h-4 w-[22px]", className)} aria-hidden="true">
      <rect x="0" y="3" width="4" height="13" rx="1" className="fill-verified" />
      <rect x="6" y="6" width="4" height="10" rx="1" className="fill-replay" />
      <rect x="12" y="1" width="4" height="15" rx="1" className="fill-live" />
      <rect x="18.5" y="9.5" width="3" height="6" rx="0.75" className="fill-none stroke-live" strokeOpacity="0.55" />
    </svg>
  );
}

export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("flex items-center gap-2 font-semibold tracking-[-0.02em]", className)}>
      <Mark />
      Gapless
    </span>
  );
}
