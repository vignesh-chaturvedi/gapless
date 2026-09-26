import { useQuery } from "@tanstack/react-query";

import { api } from "@/lib/api";
import { useFeed } from "@/lib/feed";
import type { Incident } from "@/lib/types";

/** Persisted incidents from the API, overlaid with live updates from the feed. */
export function useIncidents() {
  const query = useQuery({ queryKey: ["incidents"], queryFn: () => api.incidents(100), refetchInterval: 15_000 });
  const live = useFeed((s) => s.incidents);
  const byId = new Map<number, Incident>();
  for (const incident of query.data ?? []) byId.set(incident.id, incident);
  for (const incident of live) byId.set(incident.id, incident);
  const incidents = [...byId.values()].sort((a, b) => b.id - a.id);
  return { incidents, query };
}

export function useIncident(id: number) {
  const live = useFeed((s) => s.incidents.find((i) => i.id === id));
  const query = useQuery({ queryKey: ["incident", id], queryFn: () => api.incident(id), enabled: !live, retry: 1 });
  return { incident: live ?? query.data, query };
}

export type IncidentTone = "gap" | "replay" | "verified" | "idle";

export function incidentStatus(incident: Incident): { label: string; tone: IncidentTone } {
  const v = incident.verification;
  if (incident.status === "open") return { label: "Open", tone: "gap" };
  // Slots older than the replay horizon can't be recovered; that outweighs a clean verification.
  if (incident.unrecoverable && v?.status !== "pending" && v?.status !== "running") {
    return { label: "Partly lost", tone: "gap" };
  }
  if (v?.status === "done") {
    const kind = v.report?.verdict.kind;
    if (kind === "complete") return { label: "Verified", tone: "verified" };
    if (kind === "repaired") return { label: "Repaired", tone: "replay" };
    return { label: kind === "incomplete" ? "Incomplete" : "Inconclusive", tone: "gap" };
  }
  if (v?.status === "failed") return { label: "Check failed", tone: "gap" };
  if (v?.status === "pending" || v?.status === "running") return { label: "Verifying", tone: "replay" };
  if (v?.status === "skipped") return { label: "Recovered", tone: "idle" };
  return { label: "Recovered", tone: "replay" };
}
