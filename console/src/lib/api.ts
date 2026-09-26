import type { Incident } from "@/lib/types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: { "content-type": "application/json", ...init?.headers },
  });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) {
    const message = (body as { error?: string }).error ?? `${response.status} ${response.statusText}`;
    throw new Error(message);
  }
  return body as T;
}

export const api = {
  incidents: (limit = 50) => request<Incident[]>(`/api/incidents?limit=${limit}`),
  incident: (id: number) => request<Incident>(`/api/incidents/${id}`),

  /** Kill our stream through Solami's account API (a client cut offline). */
  kill: (holdSecs?: number) =>
    request<{ ok: boolean; method: "kill" | "cut"; connId?: string }>("/api/chaos/kill", {
      method: "POST",
      body: JSON.stringify({ holdSecs }),
    }),
  cut: (holdSecs?: number) =>
    request<{ ok: boolean }>("/api/chaos/cut", { method: "POST", body: JSON.stringify({ holdSecs }) }),
  slow: (perUpdateMs: number | null) =>
    request<{ ok: boolean; perUpdateMs: number | null }>("/api/chaos/slow", {
      method: "POST",
      body: JSON.stringify({ perUpdateMs }),
    }),
  patch: (enabled: boolean) =>
    request<{ ok: boolean; handoffPatch: boolean }>("/api/chaos/patch", {
      method: "POST",
      body: JSON.stringify({ enabled }),
    }),
};
