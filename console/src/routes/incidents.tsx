import { Flame, RotateCw } from "lucide-react";
import { Link } from "react-router";

import { IncidentBadge } from "@/components/incident-badge";
import { useCommandMenu } from "@/lib/command-menu";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import * as fmt from "@/lib/format";
import { useIncidents } from "@/lib/incidents";
import type { Incident } from "@/lib/types";

function Verification({ incident }: { incident: Incident }) {
  const v = incident.verification;
  const r = v?.report;
  if (r) {
    return (
      <span className="num">
        <span className={r.verdict.kind === "complete" ? "text-verified" : "text-gap"}>{fmt.int(r.matched)}</span>
        <span className="text-faint"> / {fmt.int(r.expected)}</span>
      </span>
    );
  }
  if (v?.status === "pending") return <span className="text-muted-foreground">waiting to finalize</span>;
  if (v?.status === "running") return <span className="text-muted-foreground">checking…</span>;
  if (v?.status === "failed") return <span className="text-gap">failed</span>;
  if (v?.status === "skipped") return <span className="text-faint">nothing missed</span>;
  return <span className="text-faint">{fmt.NONE}</span>;
}

export function Incidents() {
  const { incidents, query } = useIncidents();
  const setCommands = useCommandMenu((s) => s.setOpen);
  const loading = query.isPending && incidents.length === 0;
  const failed = query.isError && incidents.length === 0;

  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 py-6 lg:px-6">
      <div>
        <h1 className="text-xl font-semibold tracking-[-0.02em]">Incidents</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Every outage: why it happened, what was replayed, what the handoff patch recovered, and what verification found.
        </p>
      </div>

      <div className="overflow-hidden rounded-lg border border-hairline bg-panel">
        {loading ? (
          <div className="flex flex-col gap-4 p-4" aria-label="Loading incidents">
            {Array.from({ length: 4 }, (_, i) => (
              <div key={i} className="flex items-center gap-4">
                <Skeleton className="h-4 w-8" />
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-4 flex-1" />
                <Skeleton className="h-4 w-24" />
              </div>
            ))}
          </div>
        ) : failed ? (
          <div className="flex flex-col items-start gap-3 p-6">
            <p className="font-medium">Couldn't load the incident history</p>
            <p className="text-sm text-muted-foreground">{(query.error as Error).message}. Is gapless-server running?</p>
            <Button variant="outline" size="sm" onClick={() => query.refetch()}>
              <RotateCw aria-hidden="true" />
              Try again
            </Button>
          </div>
        ) : incidents.length === 0 ? (
          <div className="flex flex-col items-start gap-3 p-6">
            <p className="font-medium">No incidents yet</p>
            <p className="max-w-[60ch] text-sm text-muted-foreground">
              The stream hasn't dropped. Break it on purpose to watch Gapless replay the gap, patch the handoff and
              verify the result.
            </p>
            <Button variant="outline" size="sm" onClick={() => setCommands(true)}>
              <Flame aria-hidden="true" className="text-gap" />
              Break the stream
            </Button>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[56rem] text-sm">
              <thead>
                <tr className="border-b border-hairline text-left">
                  {["#", "Status", "Cause", "Gap", "Replayed", "Dupes", "Patch", "Verified", "Opened"].map((h, i) => (
                    <th key={h} scope="col" className={`label px-4 py-2.5 font-medium ${i >= 3 && i <= 7 ? "text-right" : ""}`}>
                      {h}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody className="divide-y divide-hairline">
                {incidents.map((incident) => (
                  <tr key={incident.id} className="transition-colors duration-100 hover:bg-muted/50">
                    <td className="px-4 py-3">
                      <Link
                        to={`/incidents/${incident.id}`}
                        className="num rounded-sm font-medium underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                      >
                        {incident.id}
                      </Link>
                    </td>
                    <td className="px-4 py-3">
                      <IncidentBadge incident={incident} />
                    </td>
                    <td className="max-w-[20rem] px-4 py-3">
                      <span className="block truncate" title={incident.detail}>
                        {fmt.sentence(incident.reason.text)}
                      </span>
                      {incident.chaos && <span className="block truncate text-xs text-faint">{fmt.sentence(incident.chaos)}</span>}
                    </td>
                    <td className="num px-4 py-3 text-right">
                      {incident.gap ? `${fmt.int(incident.gap.last - incident.gap.first + 1)} slots` : fmt.NONE}
                      {incident.unrecoverable && (
                        <span className="block text-xs text-gap">
                          {fmt.int(incident.unrecoverable.last - incident.unrecoverable.first + 1)} lost
                        </span>
                      )}
                    </td>
                    <td className="num px-4 py-3 text-right">{fmt.int(incident.replayed)}</td>
                    <td className="num px-4 py-3 text-right">{fmt.int(incident.duplicates)}</td>
                    <td className="num px-4 py-3 text-right">
                      {incident.patch ? (
                        <span className={incident.patch.recovered > 0 ? "text-replay" : "text-muted-foreground"}>
                          +{fmt.int(incident.patch.recovered)}
                        </span>
                      ) : (
                        <span className="text-faint">{fmt.NONE}</span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <Verification incident={incident} />
                    </td>
                    <td className="px-4 py-3 text-muted-foreground">
                      <time dateTime={new Date(incident.openedAt).toISOString()} title={new Date(incident.openedAt).toLocaleString()}>
                        {fmt.ago(incident.openedAt)}
                      </time>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
