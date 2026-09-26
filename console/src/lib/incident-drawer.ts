import { useCallback } from "react";
import { useSearchParams } from "react-router";

/**
 * The incident drawer's state lives in the URL (`?incident=12`), so it survives a reload, can be
 * linked, and Back closes it.
 */
export function useIncidentDrawer() {
  const [params, setParams] = useSearchParams();
  const raw = params.get("incident");
  const id = raw && /^\d+$/.test(raw) ? Number(raw) : null;

  const open = useCallback(
    (incident: number) =>
      setParams((current) => {
        const next = new URLSearchParams(current);
        next.set("incident", String(incident));
        return next;
      }),
    [setParams],
  );

  const close = useCallback(
    () =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);
          next.delete("incident");
          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  return { id, open, close };
}
