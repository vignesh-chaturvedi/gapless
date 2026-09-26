import { incidentStatus } from "@/lib/incidents";
import type { Incident } from "@/lib/types";
import { cn } from "@/lib/utils";

const tone = {
  gap: { text: "text-gap", dot: "bg-gap" },
  replay: { text: "text-replay", dot: "bg-replay" },
  verified: { text: "text-verified", dot: "bg-verified" },
  idle: { text: "text-muted-foreground", dot: "bg-faint" },
};

export function IncidentBadge({ incident, className }: { incident: Incident; className?: string }) {
  const status = incidentStatus(incident);
  return (
    <span className={cn("inline-flex items-center gap-1.5 text-xs font-medium", tone[status.tone].text, className)}>
      <span aria-hidden="true" className={cn("size-1.5 rounded-full", tone[status.tone].dot)} />
      {status.label}
    </span>
  );
}
