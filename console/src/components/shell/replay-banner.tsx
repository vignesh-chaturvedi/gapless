import { History } from "lucide-react";

import { useFeed } from "@/lib/feed";

const REPO = "https://github.com/vignesh-chaturvedi/gapless";

/** Without a Solami key the server plays the recorded fixture: say so on every page, so nobody takes it for live. */
export function ReplayBanner() {
  const offline = useFeed((s) => s.snapshot?.mode === "offline");
  if (!offline) return null;
  return (
    <div role="note" className="border-b border-replay/30 bg-replay/10">
      <p className="flex items-start gap-2 px-4 py-2 text-xs text-pretty text-muted-foreground sm:items-center lg:px-6">
        <History aria-hidden="true" className="mt-0.5 size-3.5 shrink-0 text-replay sm:mt-0" />
        <span>
          <span className="font-medium text-foreground">Recorded mainnet replay, not live.</span> This server has no Solami key, so it loops 150 s of
          Pump.fun traffic recorded through Solami; outages, replays and verification run for real against that recording. For the live stream, run it
          with your own key:{" "}
          <a href={REPO} className="font-medium text-foreground underline underline-offset-2 hover:text-verified focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
            github.com/vignesh-chaturvedi/gapless
          </a>
          .
        </span>
      </p>
    </div>
  );
}
