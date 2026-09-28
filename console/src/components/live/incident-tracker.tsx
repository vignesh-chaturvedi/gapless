import { ArrowRight, PanelRightOpen } from "lucide-react";
import type { ReactNode } from "react";

import { IncidentBadge } from "@/components/incident-badge";
import { Panel } from "@/components/panel";
import { RollingNumber } from "@/components/rolling-number";
import { Button } from "@/components/ui/button";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import { useIncidentDrawer } from "@/lib/incident-drawer";
import type { Incident, Snapshot } from "@/lib/types";
import { useNow } from "@/lib/use-now";
import { cn } from "@/lib/utils";

type StageState = "done" | "active" | "pending" | "failed" | "skipped";
type Tone = "gap" | "replay" | "verified";

interface Stage {
  key: string;
  title: string;
  tone: Tone;
  state: StageState;
  detail: ReactNode;
  /** 0..1 for a known fraction, `"wait"` for an indeterminate wait, `null` for none. */
  progress: number | "wait" | null;
}

const toneBg: Record<Tone, string> = { gap: "bg-gap", replay: "bg-replay", verified: "bg-verified" };
const toneBorder: Record<Tone, string> = { gap: "border-gap", replay: "border-replay", verified: "border-verified" };
const toneText: Record<Tone, string> = { gap: "text-gap", replay: "text-replay", verified: "text-verified" };

const clamp = (n: number) => Math.min(1, Math.max(0, n));
const Num = ({ children }: { children: ReactNode }) => <span className="num text-foreground">{children}</span>;

/** Where each stage of the loop stands, from the incident record and the live snapshot. */
function stages(incident: Incident, snapshot: Snapshot, now: number, slotMs: number | null): Stage[] {
  const state = snapshot.state;
  const open = incident.status === "open";
  const high = snapshot.metrics.highestComplete;
  const lastStep = incident.steps[incident.steps.length - 1];

  // 1. The drop.
  const holding = open && incident.steps.length === 0;
  const countdown =
    holding && state.kind === "backoff" ? Math.max(0, snapshot.stateSince + state.delayMs - now) : null;
  const offlineFor = (incident.steps[0]?.startedAt ?? incident.recoveredAt ?? now) - incident.openedAt;
  // While offline, the gap ages out of Solami's replay horizon from its oldest slot.
  const horizon = snapshot.replayHorizon ?? 3_000;
  const behind =
    holding && snapshot.tip != null && incident.resumeFrom != null ? snapshot.tip - incident.resumeFrom + 1 : null;
  const replayable = behind !== null ? horizon - behind : null;
  const drop: Stage = {
    key: "drop",
    title: "Disconnected",
    tone: "gap",
    state: holding ? "active" : "done",
    progress:
      countdown !== null && state.kind === "backoff" && state.delayMs > 0 ? 1 - countdown / state.delayMs : null,
    detail: (
      <>
        {fmt.sentence(incident.reason.text)}.{" "}
        {countdown !== null ? (
          <>
            Reconnecting in <Num>{fmt.duration(countdown)}</Num>.
            {replayable !== null &&
              (replayable > 0 ? (
                <span className="block">
                  Replayable for{" "}
                  <Num>{slotMs ? `~${fmt.duration(replayable * slotMs)}` : `${fmt.int(replayable)} slots`}</Num> more.
                </span>
              ) : (
                <span className="block text-gap">
                  <Num>{fmt.int(-replayable)}</Num> slots already past Solami's replay horizon.
                </span>
              ))}
          </>
        ) : (
          <>
            Offline <Num>{fmt.duration(offlineFor)}</Num>
          </>
        )}
      </>
    ),
  };

  // 2. The replay.
  const replayingNow = state.kind === "replaying" && state.incident === incident.sessionIncident;
  const replay: Stage = {
    key: "replay",
    title: incident.steps.length > 1 ? `Replayed in ${incident.steps.length} steps` : "Replayed",
    tone: "replay",
    state: incident.recoveredAt ? "done" : incident.steps.length ? "active" : "pending",
    progress:
      replayingNow && high != null
        ? clamp((high - state.fromSlot) / Math.max(1, state.targetSlot - state.fromSlot))
        : null,
    detail: incident.recoveredAt ? (
      <>
        {incident.unrecoverable && (
          <span className="block text-gap">
            <Num>{fmt.int(incident.unrecoverable.last - incident.unrecoverable.first + 1)}</Num> slots lost: older than
            Solami's replay horizon.
          </span>
        )}
        <Num>{fmt.int(incident.replayed)}</Num> transactions
        {incident.gap && (
          <>
            {" "}
            from a <Num>{fmt.int(incident.gap.last - incident.gap.first + 1)}</Num>-slot gap
          </>
        )}
        ; <Num>{fmt.int(incident.duplicates)}</Num> duplicate{incident.duplicates === 1 ? "" : "s"} dropped.
      </>
    ) : lastStep ? (
      <>
        from_slot <Num>{fmt.slot(lastStep.fromSlot)}</Num> to <Num>{fmt.slot(lastStep.targetSlot)}</Num>
      </>
    ) : (
      "Resumes from the last complete slot."
    ),
  };

  // 3. The handoff patch.
  const target = lastStep?.targetSlot ?? null;
  let patch: Stage;
  if (incident.patch) {
    patch = {
      key: "patch",
      title: "Handoff patched",
      tone: "replay",
      state: incident.patch.error ? "failed" : "done",
      progress: null,
      detail: incident.patch.error ? (
        `Failed: ${incident.patch.error}`
      ) : incident.patch.recovered > 0 ? (
        <>
          Recovered <Num>{fmt.int(incident.patch.recovered)}</Num> Solami dropped at the switch to live.
        </>
      ) : (
        "Re-read the handoff slots: nothing was dropped."
      ),
    };
  } else if (incident.recoveredAt && (incident.verification || !snapshot.controls.handoffPatch)) {
    patch = {
      key: "patch",
      title: "Handoff patch",
      tone: "replay",
      state: "skipped",
      progress: null,
      detail: incident.steps.length ? "Off. Verification will show what Solami dropped." : "Not needed: nothing replayed.",
    };
  } else {
    patch = {
      key: "patch",
      title: "Handoff patch",
      tone: "replay",
      state: incident.recoveredAt ? "active" : "pending",
      progress: incident.recoveredAt && target != null && high != null ? clamp((high - target) / 52) : null,
      detail:
        incident.recoveredAt && target != null ? (
          <>
            Re-reads slots <Num>{fmt.slot(target)}</Num>+ once the stream is past them.
          </>
        ) : (
          "Re-reads the slots around the switch back to live."
        ),
    };
  }

  // 4. Verification against RPC.
  const v = incident.verification;
  const report = v?.report ?? null;
  let verify: Stage;
  if (!v) {
    verify = { key: "verify", title: "Verified", tone: "verified", state: "pending", progress: null, detail: "Checked against RPC once the slots finalize." };
  } else if (
    v.status === "pending" &&
    v.range &&
    snapshot.finalized != null &&
    snapshot.finalized >= v.range.last &&
    (high ?? 0) < v.range.last
  ) {
    // Final on chain, but our own stream hasn't delivered the end of the range yet.
    verify = {
      key: "verify",
      title: "Waiting for the stream",
      tone: "verified",
      state: "active",
      progress: "wait",
      detail: (
        <>
          Slot <Num>{fmt.slot(v.range.last)}</Num> is final; the stream is{" "}
          <Num>{fmt.int(v.range.last - (high ?? v.range.first))}</Num> slots short of it. Checking before it arrives
          would count late transactions as missing.
        </>
      ),
    };
  } else if (v.status === "pending" && v.range) {
    const finalized = snapshot.finalized;
    const left = finalized != null ? Math.max(0, v.range.last - finalized) : null;
    verify = {
      key: "verify",
      title: "Waiting to finalize",
      tone: "verified",
      state: "active",
      progress: finalized != null ? clamp((finalized - v.range.first) / Math.max(1, v.range.last - v.range.first)) : "wait",
      detail:
        left !== null ? (
          <>
            Slot <Num>{fmt.slot(v.range.last)}</Num> finalizes in <Num>{fmt.int(left)}</Num> slots
            {slotMs ? (
              <>
                {" "}
                (<Num>~{fmt.duration(left * slotMs)}</Num>)
              </>
            ) : null}
            .
          </>
        ) : (
          "Waiting for the incident's slots to finalize."
        ),
    };
  } else if (v.status === "running" || v.status === "pending") {
    verify = {
      key: "verify",
      title: "Verifying",
      tone: "verified",
      state: "active",
      progress: "wait",
      detail: v.range ? (
        <>
          Checking <Num>{fmt.int(v.range.last - v.range.first + 1)}</Num> slots against RPC.
        </>
      ) : (
        "Checking against RPC."
      ),
    };
  } else if (v.status === "done" && report) {
    const kind = report.verdict.kind;
    verify = {
      key: "verify",
      title: kind === "complete" ? "Verified complete" : kind === "repaired" ? "Verified, repaired" : "Verified: gaps found",
      tone: "verified",
      state: kind === "complete" || kind === "repaired" ? "done" : "failed",
      progress: null,
      detail: (
        <>
          <span className={cn("num block text-base leading-7 font-medium tracking-tight whitespace-nowrap lg:text-lg", kind === "complete" ? "text-verified" : "text-foreground")}>
            <RollingNumber value={fmt.int(report.matched)} />
            <span className="text-faint"> / {fmt.int(report.expected)}</span>
          </span>
          {kind === "complete"
            ? "expected transactions delivered."
            : kind === "repaired"
              ? `${fmt.int(report.repaired.length)} missing, fetched from RPC.`
              : `${fmt.int(report.missing.length)} missing.`}
        </>
      ),
    };
  } else if (v.status === "skipped") {
    verify = { key: "verify", title: "Nothing to verify", tone: "verified", state: "skipped", progress: null, detail: "No slots were missed." };
  } else {
    verify = { key: "verify", title: "Check failed", tone: "verified", state: "failed", progress: null, detail: v.error ?? "Verification failed." };
  }

  return [drop, replay, patch, verify];
}

function Node({ stage }: { stage: Stage }) {
  const { state, tone } = stage;
  return (
    <span aria-hidden="true" className="relative flex size-3 shrink-0 items-center justify-center">
      {state === "active" && (
        <span className={cn("absolute inset-0 rounded-full opacity-50 motion-safe:animate-ping [animation-duration:1.6s]", toneBg[tone])} />
      )}
      <span
        className={cn(
          "relative size-3 rounded-full border-2",
          state === "done" && [toneBg[tone], toneBorder[tone]],
          state === "active" && [toneBorder[tone], "bg-panel"],
          state === "failed" && "border-gap bg-gap",
          state === "pending" && "border-faint/50",
          state === "skipped" && "border-dashed border-faint/70",
        )}
      />
    </span>
  );
}

function Progress({ value, tone }: { value: number | "wait"; tone: Tone }) {
  if (value === "wait") {
    return (
      <div
        aria-hidden="true"
        className="mt-2.5 h-1 overflow-hidden rounded-full bg-muted motion-safe:animate-[hatch-slide_0.8s_linear_infinite]"
        style={{
          backgroundImage: `repeating-linear-gradient(135deg, color-mix(in oklch, var(--${tone}) 55%, transparent) 0 4px, transparent 4px 8px)`,
        }}
      />
    );
  }
  return (
    <div aria-hidden="true" className="mt-2.5 h-1 overflow-hidden rounded-full bg-muted">
      <div
        className={cn("h-full origin-left rounded-full transition-transform duration-700 ease-crisp", toneBg[tone])}
        style={{ transform: `scaleX(${Math.max(0.02, value)})` }}
      />
    </div>
  );
}

const stateWord: Record<StageState, string> = {
  done: "done",
  active: "in progress",
  pending: "not started",
  failed: "failed",
  skipped: "skipped",
};

/** The recovery loop for the newest incident, stage by stage, as it happens. */
export function IncidentTracker({ onBreak }: { onBreak?: () => void }) {
  const incident = useFeed((s) => s.incidents[0]);
  const snapshot = useFeed((s) => s.snapshot);
  const slotMs = useFeed((s) => s.slotMs);
  const now = useNow(500);
  const drawer = useIncidentDrawer();

  if (!snapshot) return null;

  if (!incident) {
    return (
      <Panel title="Recovery loop" bodyClassName="flex flex-col items-start gap-3 p-5">
        <ol className="grid w-full grid-cols-4 gap-2" aria-hidden="true">
          {["Disconnected", "Replayed", "Handoff patched", "Verified"].map((t, i) => (
            <li key={t} className="flex items-center gap-2">
              <span className="size-3 shrink-0 rounded-full border-2 border-faint/40" />
              {i < 3 && <span className="h-px flex-1 bg-hairline" />}
            </li>
          ))}
        </ol>
        <div>
          <p className="font-medium">No incidents yet</p>
          <p className="mt-1 max-w-prose text-sm text-muted-foreground">
            Break the stream and watch Gapless replay the gap, drop the duplicates, patch the handoff and prove against RPC
            that nothing was lost.
          </p>
        </div>
        {onBreak && (
          <Button variant="outline" size="sm" onClick={onBreak}>
            Open the chaos panel
            <ArrowRight aria-hidden="true" />
          </Button>
        )}
      </Panel>
    );
  }

  const list = stages(incident, snapshot, now, slotMs);
  const open = incident.status === "open" || incident.verification?.status === "pending" || incident.verification?.status === "running";

  return (
    <Panel
      title={open ? "Recovery loop · in progress" : "Recovery loop · last incident"}
      aside={
        <Button
          variant="ghost"
          size="sm"
          className="-mr-2 h-7 text-muted-foreground pointer-coarse:h-10"
          onClick={() => drawer.open(incident.id)}
        >
          <PanelRightOpen aria-hidden="true" />
          Details
        </Button>
      }
      bodyClassName="flex flex-col gap-5 p-5"
    >
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <h3 className="text-base font-semibold tracking-tight">
          Incident <span className="num">#{incident.id}</span>
        </h3>
        <IncidentBadge incident={incident} />
        <span className="text-sm text-muted-foreground">
          {incident.chaos ? fmt.sentence(incident.chaos) : fmt.sentence(incident.reason.text)} ·{" "}
          <time className="num" dateTime={new Date(incident.openedAt).toISOString()}>
            {fmt.clock(incident.openedAt)}
          </time>
        </span>
      </div>

      <ol className="grid gap-5 sm:grid-cols-4 sm:gap-0">
        {list.map((stage, i) => (
          <li
            key={stage.key}
            className="relative grid min-w-0 grid-cols-[0.75rem_minmax(0,1fr)] items-start gap-x-3 sm:block sm:pr-5"
            aria-label={`${stage.title}: ${stateWord[stage.state]}`}
          >
            <div className="mt-1 flex items-center gap-2 sm:mt-0">
              <Node stage={stage} />
              {i < list.length - 1 && (
                <span aria-hidden="true" className="relative hidden h-px flex-1 bg-hairline sm:block">
                  <span
                    className={cn(
                      "absolute inset-0 origin-left transition-transform duration-700 ease-crisp",
                      toneBg[stage.tone],
                    )}
                    style={{ transform: `scaleX(${stage.state === "done" || stage.state === "skipped" ? 1 : 0})` }}
                  />
                </span>
              )}
            </div>
            <p
              className={cn(
                "text-sm font-medium sm:mt-3",
                stage.state === "pending" && "text-muted-foreground",
                stage.state === "active" && toneText[stage.tone],
                stage.state === "failed" && "text-gap",
              )}
            >
              {stage.title}
            </p>
            <div className="col-start-2 mt-1 text-xs leading-5 text-pretty text-muted-foreground">{stage.detail}</div>
            {stage.progress !== null && stage.state === "active" && (
              <div className="col-start-2">
                <Progress value={stage.progress} tone={stage.tone} />
              </div>
            )}
          </li>
        ))}
      </ol>
    </Panel>
  );
}
