import { ArrowUpRight } from "lucide-react";
import { Link } from "react-router";

import { IncidentBadge } from "@/components/incident-badge";
import { IncidentDetail } from "@/components/incident-detail";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Skeleton } from "@/components/ui/skeleton";
import * as fmt from "@/lib/format";
import { useIncidentDrawer } from "@/lib/incident-drawer";
import { useIncident } from "@/lib/incidents";

function Body({ id }: { id: number }) {
  const { incident, query } = useIncident(id);
  if (incident) {
    return (
      <>
        <SheetHeader className="gap-1.5 border-b border-hairline pr-12">
          <div className="flex flex-wrap items-center gap-3">
            <SheetTitle className="text-lg font-semibold tracking-tight">
              Incident <span className="num">#{incident.id}</span>
            </SheetTitle>
            <IncidentBadge incident={incident} />
          </div>
          <SheetDescription>
            {incident.chaos ? fmt.sentence(incident.chaos) : fmt.sentence(incident.reason.text)} ·{" "}
            <span className="num">{fmt.clock(incident.openedAt)}</span>
          </SheetDescription>
          <Link
            to={`/incidents/${incident.id}`}
            className="mt-1 inline-flex w-fit items-center gap-1 rounded-sm text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            Open as a page
            <ArrowUpRight aria-hidden="true" className="size-3" />
          </Link>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-6">
          <IncidentDetail incident={incident} stacked />
        </div>
      </>
    );
  }
  return (
    <>
      <SheetHeader className="border-b border-hairline">
        <SheetTitle>
          Incident <span className="num">#{id}</span>
        </SheetTitle>
        <SheetDescription>{query.isError ? (query.error as Error).message : "Loading…"}</SheetDescription>
      </SheetHeader>
      {!query.isError && (
        <div className="flex flex-col gap-4 px-4" aria-hidden="true">
          <Skeleton className="h-24" />
          <Skeleton className="h-72" />
        </div>
      )}
    </>
  );
}

/** Incident details over whatever page is open, driven by `?incident=` in the URL. */
export function IncidentDrawer() {
  const { id, close } = useIncidentDrawer();
  return (
    <Sheet open={id !== null} onOpenChange={(open) => !open && close()}>
      <SheetContent side="right" className="gap-0 data-[side=right]:w-full data-[side=right]:sm:max-w-xl">
        {id !== null && <Body id={id} />}
      </SheetContent>
    </Sheet>
  );
}
