import { ExternalLink } from "lucide-react";
import type { ReactNode } from "react";

import { Panel } from "@/components/panel";
import * as fmt from "@/lib/format";
import type { Incident, VerificationReport } from "@/lib/types";
import { cn } from "@/lib/utils";

type Tone = "gap" | "replay" | "verified" | "idle";
const rail: Record<Tone, string> = {
  gap: "border-gap bg-gap",
  replay: "border-replay bg-replay",
  verified: "border-verified bg-verified",
  idle: "border-faint bg-transparent",
};

function Event({ tone, title, at, children }: { tone: Tone; title: string; at?: number | null; children?: ReactNode }) {
  return (
    <li className="relative grid grid-cols-[1rem_1fr] gap-3 pb-6 last:pb-0">
      <span aria-hidden="true" className={cn("relative z-10 mt-1.5 size-2.5 rounded-full border-2", rail[tone])} />
      <div className="min-w-0">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3">
          <h3 className="font-medium">{title}</h3>
          {at ? <time className="num text-xs text-faint">{fmt.clock(at)}</time> : null}
        </div>
        {children && <div className="mt-1 text-sm text-muted-foreground">{children}</div>}
      </div>
    </li>
  );
}

function Fact({ term, children }: { term: string; children: ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4 border-b border-hairline py-2.5 last:border-b-0">
      <dt className="text-sm text-muted-foreground">{term}</dt>
      <dd className="num text-right text-sm">{children}</dd>
    </div>
  );
}

function Verdict({ report }: { report: VerificationReport }) {
  const complete = report.verdict.kind === "complete";
  return (
    <div className={cn("rounded-md border px-4 py-3", complete ? "border-verified/40" : "border-gap/40")}>
      <p className={cn("label", complete ? "text-verified" : "text-gap")}>
        {report.verdict.kind === "complete"
          ? "Complete"
          : report.verdict.kind === "repaired"
            ? `Repaired · ${report.verdict.repaired} fetched from RPC`
            : report.verdict.kind === "incomplete"
              ? `Incomplete · ${report.verdict.missing} missing`
              : "Inconclusive"}
      </p>
      <p className="num mt-1.5 text-2xl font-medium tracking-tight">
        {fmt.int(report.matched)} <span className="text-faint">/ {fmt.int(report.expected)}</span>
      </p>
      <p className="mt-1 text-xs text-muted-foreground">
        expected transactions delivered, from <span className="num">{report.source}</span>
      </p>
    </div>
  );
}

/** Each slot of a verified range: matched in blue, short in red, skipped slots as dots. */
export function VerificationStrip({ report }: { report: VerificationReport }) {
  const slots = report.perSlot;
  if (slots.length === 0) return null;
  const peak = Math.max(1, ...slots.map((s) => s.expected));
  const W = 4;
  const H = 40;
  const short = slots.filter((s) => s.matched < s.expected).length;
  return (
    <figure className="flex flex-col gap-2">
      <svg
        viewBox={`0 0 ${slots.length * W} ${H}`}
        preserveAspectRatio="none"
        className="block h-10 w-full"
        role="img"
        aria-label={`${fmt.plural(slots.length, "slot")} checked; ${short ? `${fmt.plural(short, "slot")} short` : "every slot complete"}.`}
      >
        {slots.map((s, i) => {
          if (!s.hasBlock) {
            return <rect key={s.slot} x={i * W + 1} y={H - 2} width={2} height={2} fill="var(--faint)" opacity={0.6} />;
          }
          const h = Math.max(2, (H - 2) * Math.sqrt(s.expected / peak));
          const ok = s.matched >= s.expected;
          return (
            <rect key={s.slot} x={i * W + 0.5} y={H - h} width={W - 1} height={h} fill={ok ? "var(--verified)" : "var(--gap)"}>
              <title>{`Slot ${fmt.slot(s.slot)}: ${s.matched} of ${s.expected}`}</title>
            </rect>
          );
        })}
      </svg>
      <figcaption className="flex justify-between text-xs text-muted-foreground">
        <span className="num">{fmt.slot(slots[0].slot)}</span>
        <span>
          {fmt.int(report.slotsWithBlocks)} blocks, {fmt.int(report.skippedSlots)} skipped
        </span>
        <span className="num">{fmt.slot(slots[slots.length - 1].slot)}</span>
      </figcaption>
    </figure>
  );
}

/** Everything about one incident: timeline, verdict, numbers and evidence. `stacked` fits a drawer. */
export function IncidentDetail({ incident, stacked = false }: { incident: Incident; stacked?: boolean }) {
  const v = incident.verification;
  const report = v?.report ?? null;
  const gapSlots = incident.gap ? incident.gap.last - incident.gap.first + 1 : null;
  const firstStep = incident.steps[0];
  const offlineMs = firstStep ? firstStep.startedAt - incident.openedAt : null;

  return (
    <div className={cn("grid gap-4", !stacked && "lg:grid-cols-[1fr_22rem]")}>
      {stacked && report && <Verdict report={report} />}
      <Panel title="Timeline" bodyClassName="p-5">
        <ol className="relative before:absolute before:top-2 before:bottom-2 before:left-[4.5px] before:w-px before:bg-hairline">
          <Event tone="gap" title="Disconnected" at={incident.openedAt}>
            {fmt.sentence(incident.reason.text)}
            {!incident.detail.toLowerCase().includes(incident.reason.text.toLowerCase()) && (
              <span className="text-faint"> ({incident.detail})</span>
            )}
            . Last complete slot{" "}
            <span className="num text-foreground">{fmt.slot(incident.lastCompleteSlot)}</span>; resuming from{" "}
            <span className="num text-foreground">{fmt.slot(incident.resumeFrom)}</span>.
          </Event>
          {offlineMs !== null && offlineMs > 3_000 && (
            <Event tone="gap" title={`Offline for ${fmt.duration(offlineMs)}`}>
              {incident.chaos ? `${fmt.sentence(incident.chaos)}.` : "Waiting to reconnect."} The chain kept producing
              slots.
            </Event>
          )}
          {incident.steps.map((step) => (
            <Event
              key={step.attempt}
              tone="replay"
              title={incident.steps.length > 1 ? `Replay step ${step.attempt}` : "Replaying"}
              at={step.startedAt}
            >
              from_slot <span className="num text-foreground">{fmt.slot(step.fromSlot)}</span> to the tip at{" "}
              <span className="num text-foreground">{fmt.slot(step.targetSlot)}</span>
              {step.transactions > 0 && <>, {fmt.plural(step.transactions, "transaction")}</>}
              {step.ended && (
                <>
                  . Ended early: <span className="text-foreground">{step.ended.text}</span>
                </>
              )}
              .
            </Event>
          ))}
          {incident.recoveredAt && (
            <Event tone="replay" title={`Recovered in ${fmt.duration(incident.durationMs)}`} at={incident.recoveredAt}>
              Caught up to the live tip. <span className="num text-foreground">{fmt.int(incident.replayed)}</span>{" "}
              replayed, {fmt.plural(incident.duplicates, "duplicate")} dropped.
            </Event>
          )}
          {incident.patch && (
            <Event tone="replay" title="Handoff patch">
              Re-read slots <span className="num text-foreground">{fmt.slot(incident.patch.slots.first)}</span>–
              <span className="num text-foreground">{fmt.slot(incident.patch.slots.last)}</span> around the switch back to
              live and recovered{" "}
              <span className={cn("num", incident.patch.recovered ? "text-replay" : "text-foreground")}>
                {fmt.int(incident.patch.recovered)}
              </span>{" "}
              transaction{incident.patch.recovered === 1 ? "" : "s"} Solami dropped.
              {incident.patch.error && <span className="text-gap"> Patch error: {incident.patch.error}.</span>}
            </Event>
          )}
          {v && (
            <Event
              tone={v.status === "done" && report?.verdict.kind === "complete" ? "verified" : v.status === "failed" ? "gap" : "idle"}
              title={
                v.status === "done"
                  ? "Verified"
                  : v.status === "running"
                    ? "Verifying"
                    : v.status === "pending"
                      ? "Waiting for the slots to finalize"
                      : v.status === "skipped"
                        ? "Nothing to verify"
                        : "Verification failed"
              }
            >
              {v.range && (
                <>
                  Slots <span className="num text-foreground">{fmt.slot(v.range.first)}</span>–
                  <span className="num text-foreground">{fmt.slot(v.range.last)}</span>: the gap plus the handoff slots after
                  it.{" "}
                </>
              )}
              {v.error}
              {report &&
                (report.source === "fixture" ? (
                  <>Checked against the fixture's own record of every slot in {fmt.ms(report.elapsedMs)}.</>
                ) : (
                  <>
                    {report.spotChecks.filter((c) => c.agree).length} of {report.spotChecks.length} getBlock spot checks
                    agree;{" "}
                    <span className="num">
                      {fmt.plural(report.rpc.calls, "call")}, {fmt.bytes(report.rpc.bytes)}, {fmt.ms(report.elapsedMs)}
                    </span>
                    .
                  </>
                ))}
            </Event>
          )}
        </ol>
      </Panel>

      <div className="flex flex-col gap-4">
        {!stacked && report && <Verdict report={report} />}
        {report && report.perSlot.length > 0 && (
          <Panel title="Per slot" bodyClassName="p-4">
            <VerificationStrip report={report} />
          </Panel>
        )}
        <Panel title="Numbers" bodyClassName="px-4 py-1">
          <dl>
            <Fact term="Gap">{gapSlots === null ? fmt.NONE : `${fmt.int(gapSlots)} slots`}</Fact>
            <Fact term="Replayed">{fmt.int(incident.replayed)}</Fact>
            <Fact term="Duplicates dropped">{fmt.int(incident.duplicates)}</Fact>
            <Fact term="Recovered by the patch">{incident.patch ? fmt.int(incident.patch.recovered) : fmt.NONE}</Fact>
            <Fact term="Replay steps">{fmt.int(incident.steps.length)}</Fact>
            <Fact term="Unrecoverable">
              {incident.unrecoverable
                ? `${fmt.int(incident.unrecoverable.last - incident.unrecoverable.first + 1)} slots`
                : "none"}
            </Fact>
          </dl>
        </Panel>
        {report && (report.missing.length > 0 || report.repaired.length > 0) && (
          <Panel title="Missing transactions" bodyClassName="divide-y divide-hairline">
            {report.missing.map((m) => (
              <a
                key={m.signature}
                href={`https://solscan.io/tx/${m.signature}`}
                target="_blank"
                rel="noreferrer"
                className="flex items-center justify-between gap-3 px-4 py-2 text-sm hover:bg-muted/50 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <span className="num truncate" title={m.signature}>
                  {fmt.short(m.signature, 6)}
                </span>
                <span className="flex items-center gap-2 text-xs text-muted-foreground">
                  {m.cause === "replay_handoff" ? "handoff" : m.cause ?? ""} · slot {fmt.slot(m.slot)}
                  <ExternalLink aria-hidden="true" className="size-3" />
                </span>
              </a>
            ))}
          </Panel>
        )}
      </div>
    </div>
  );
}

