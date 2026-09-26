import { Link } from "react-router";

import { IncidentBadge } from "@/components/incident-badge";
import { Panel, Readout } from "@/components/panel";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import { useIncidents } from "@/lib/incidents";

const SOURCES = [
  {
    name: "getTransactionsForAddress",
    role: "Expected set",
    body: "Solami's filtered address history: every successful transaction touching the program in a slot range, including through lookup tables. It's cheap enough to run on every slot.",
  },
  {
    name: "getBlock",
    role: "Independent check",
    body: "Full blocks with the stream's filter applied by Gapless itself. Used as spot checks on a few slots per incident, and on every slot with a difference.",
  },
];

export function Verify() {
  const snapshot = useFeed((s) => s.snapshot);
  const { incidents } = useIncidents();
  const verified = incidents.filter((i) => i.verification?.report);
  const matched = verified.reduce((sum, i) => sum + (i.verification?.report?.matched ?? 0), 0);
  const expected = verified.reduce((sum, i) => sum + (i.verification?.report?.expected ?? 0), 0);
  const behind =
    snapshot?.metrics.highestComplete && snapshot.verifiedThrough
      ? snapshot.metrics.highestComplete - snapshot.verifiedThrough
      : null;

  return (
    <div className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 py-6 lg:px-6">
      <div>
        <h1 className="text-xl font-semibold tracking-[-0.02em]">Verification</h1>
        <p className="mt-1 max-w-[70ch] text-sm text-muted-foreground">
          Gapless doesn't claim a stream is complete; it checks. Once slots finalize, it rebuilds what should have
          arrived and compares it with what did, transaction by transaction.
        </p>
      </div>

      <section aria-label="Verification readouts" className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-hairline bg-hairline lg:grid-cols-4">
        <div className="bg-panel px-4 py-3.5">
          <Readout label="Verified to slot" value={fmt.slot(snapshot?.verifiedThrough)} tone="verified" />
        </div>
        <div className="bg-panel px-4 py-3.5">
          <Readout label="Behind the stream" value={behind === null ? fmt.NONE : fmt.int(behind)} unit="slots" hint="finalization takes ~32 slots" />
        </div>
        <div className="bg-panel px-4 py-3.5">
          <Readout label="Incidents verified" value={fmt.int(verified.length)} hint={`of ${fmt.int(incidents.length)}`} />
        </div>
        <div className="bg-panel px-4 py-3.5">
          <Readout
            label="Recovered transactions checked"
            value={expected ? `${fmt.int(matched)} / ${fmt.int(expected)}` : fmt.NONE}
            tone={expected && matched === expected ? "verified" : "default"}
          />
        </div>
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title="Two sources of truth" bodyClassName="divide-y divide-hairline">
          {SOURCES.map((s) => (
            <div key={s.name} className="px-4 py-4">
              <p className="label">{s.role}</p>
              <p className="num mt-1 text-sm">{s.name}</p>
              <p className="mt-1.5 text-sm text-muted-foreground">{s.body}</p>
            </div>
          ))}
        </Panel>
        <Panel title="Incident reports" bodyClassName="divide-y divide-hairline">
          {verified.length === 0 ? (
            <p className="p-4 text-sm text-muted-foreground">
              No incident has been verified yet. Reports appear here after an outage recovers and its slots finalize.
            </p>
          ) : (
            verified.slice(0, 8).map((i) => {
              const r = i.verification!.report!;
              return (
                <Link
                  key={i.id}
                  to={`/incidents/${i.id}`}
                  className="flex items-center justify-between gap-4 px-4 py-3 text-sm transition-colors duration-100 hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  <span className="flex items-center gap-3">
                    <span className="num text-muted-foreground">#{i.id}</span>
                    <IncidentBadge incident={i} />
                  </span>
                  <span className="num">
                    {fmt.int(r.matched)} <span className="text-faint">/ {fmt.int(r.expected)}</span>
                  </span>
                </Link>
              );
            })
          )}
        </Panel>
      </div>
    </div>
  );
}
