import { OctagonX } from "lucide-react";

import * as fmt from "@/lib/format";

/** The stream stopped for good (a rejected key, an exhausted balance): say why and what fixes it. */
export function StoppedBanner({ reason }: { reason: string }) {
  const r = reason.toLowerCase();
  const hint =
    r.includes("api key") || r.includes("unauthenticated") || r.includes("permission") ? (
      <>
        Check <code className="num text-foreground">SOLAMI_API_KEY</code> in <code className="num text-foreground">.env</code>{" "}
        (a standard key with the Developer role), then restart gapless-server.
      </>
    ) : r.includes("balance") ? (
      "The account's streaming balance is used up. Top it up in Solami's dashboard, then restart gapless-server."
    ) : (
      <>
        Fix the cause, then restart gapless-server. To look around without a key, run it with{" "}
        <code className="num text-foreground">--offline fixtures/pumpfun-150s.bin.zst</code>.
      </>
    );
  return (
    <div role="alert" className="flex gap-3 rounded-lg border border-gap/40 bg-gap/5 px-4 py-3">
      <OctagonX aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-gap" />
      <div className="min-w-0">
        <p className="font-medium">The stream stopped: {fmt.sentence(reason)}</p>
        <p className="mt-1 text-sm text-pretty text-muted-foreground">{hint}</p>
      </div>
    </div>
  );
}
