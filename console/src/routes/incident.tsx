import { ArrowLeft } from "lucide-react";
import { Link, useParams } from "react-router";

import { IncidentBadge } from "@/components/incident-badge";
import { IncidentDetail } from "@/components/incident-detail";
import { Skeleton } from "@/components/ui/skeleton";
import { useIncident } from "@/lib/incidents";

export function IncidentPage() {
  const { id: raw } = useParams();
  const id = Number(raw);
  const { incident, query } = useIncident(id);

  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 py-6 lg:px-6">
      <Link
        to="/incidents"
        className="flex w-fit items-center gap-1.5 rounded-sm text-sm text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <ArrowLeft aria-hidden="true" className="size-4" />
        Incidents
      </Link>
      {incident ? (
        <>
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="text-xl font-semibold tracking-[-0.02em]">
              Incident <span className="num">#{incident.id}</span>
            </h1>
            <IncidentBadge incident={incident} />
          </div>
          <IncidentDetail incident={incident} />
        </>
      ) : query.isError ? (
        <div className="rounded-lg border border-hairline bg-panel p-6">
          <p className="font-medium">Incident #{raw} isn't here</p>
          <p className="mt-1 text-sm text-muted-foreground">{(query.error as Error).message}</p>
        </div>
      ) : (
        <div className="grid gap-4 lg:grid-cols-[1fr_22rem]" aria-label="Loading incident">
          <Skeleton className="h-80" />
          <Skeleton className="h-80" />
        </div>
      )}
    </div>
  );
}
