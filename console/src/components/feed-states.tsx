import { PlugZap, RotateCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { reconnectFeed, useFeed } from "@/lib/feed";

/** Shown in place of live panels when gapless-server can't be reached. */
export function ServerOffline() {
  const link = useFeed((s) => s.link);
  return (
    <div className="flex flex-col items-start gap-4 rounded-lg border border-hairline bg-panel p-6">
      <div className="flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-md border border-hairline text-gap">
          <PlugZap aria-hidden="true" className="size-4" />
        </span>
        <div>
          <h2 className="font-medium">The console can't reach gapless-server</h2>
          <p className="text-sm text-muted-foreground">
            {link === "connecting" ? "Trying again now…" : "It retries on its own. Start the server, or retry now."}
          </p>
        </div>
      </div>
      <pre className="num w-full overflow-x-auto rounded-md border border-hairline bg-background px-3 py-2.5 text-xs text-muted-foreground">
        <code>
          <span className="text-faint"># live (needs SOLAMI_API_KEY in .env)</span>
          {"\n"}cargo run -p gapless-server --release
          {"\n"}
          <span className="text-faint"># or no key: replay recorded mainnet</span>
          {"\n"}cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst
        </code>
      </pre>
      <Button variant="outline" size="sm" onClick={reconnectFeed}>
        <RotateCw aria-hidden="true" />
        Retry now
      </Button>
    </div>
  );
}

/** Row skeletons shaped like a list of transactions or log lines. */
export function RowsSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="flex flex-col gap-3 p-4" aria-hidden="true">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex items-center gap-3">
          <Skeleton className="h-3 w-14" />
          <Skeleton className="h-3 flex-1" style={{ maxWidth: `${60 + ((i * 37) % 35)}%` }} />
          <Skeleton className="h-3 w-10" />
        </div>
      ))}
    </div>
  );
}
