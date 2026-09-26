import { Link } from "react-router";

import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import type { LogLine } from "@/lib/types";
import { cn } from "@/lib/utils";

const levelMark: Record<LogLine["level"], string> = {
  info: "bg-faint",
  success: "bg-verified",
  warn: "bg-replay",
  error: "bg-gap",
};

/** What Gapless did and why, newest first. */
export function ActivityLog({ limit = 60 }: { limit?: number }) {
  const log = useFeed((s) => s.log);
  const rows = log.slice(0, limit);
  if (rows.length === 0) {
    return <p className="p-4 text-sm text-muted-foreground">Nothing has happened yet. Disconnects, replays and verifications show up here.</p>;
  }
  return (
    <ol className="flex flex-col">
      {rows.map((line, i) => (
        <li key={`${line.at}-${i}`} className="grid grid-cols-[4.25rem_auto_1fr] items-start gap-3 px-4 py-2 text-sm">
          <time className="num pt-px text-xs text-faint" dateTime={new Date(line.at).toISOString()}>
            {fmt.clock(line.at)}
          </time>
          <span aria-hidden="true" className={cn("mt-1.5 size-1.5 rounded-full", levelMark[line.level])} />
          <span className="min-w-0 text-pretty text-muted-foreground">
            <span className={cn(line.level !== "info" && "text-foreground")}>{line.text}</span>
            {line.incident !== null && line.incident > 0 && (
              <>
                {" "}
                <Link
                  to={`/incidents/${line.incident}`}
                  className="num rounded-sm text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  #{line.incident}
                </Link>
              </>
            )}
          </span>
        </li>
      ))}
    </ol>
  );
}
