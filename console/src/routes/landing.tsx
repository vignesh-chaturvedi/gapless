import { ArrowRight, ArrowDown } from "lucide-react";
import { useMemo } from "react";
import { Link } from "react-router";

import { Wordmark } from "@/components/brand/mark";
import { ThemeToggle } from "@/components/shell/top-bar";
import { StreamPill } from "@/components/stream-pill";
import { SlotTape, TapeLegend } from "@/components/tape/slot-tape";
import { Button } from "@/components/ui/button";
import { useFeed } from "@/lib/feed";
import * as fmt from "@/lib/format";
import { createPreviewTape } from "@/lib/preview-tape";
import { liveTape } from "@/lib/tape";
import { cn } from "@/lib/utils";

const LOOP = [
  {
    n: "01",
    title: "Break",
    tone: "text-gap",
    bar: "bg-gap",
    body: "Kill the stream through Solami's account API. Everything after the last complete slot is a gap.",
  },
  {
    n: "02",
    title: "Replay",
    tone: "text-replay",
    bar: "bg-replay",
    body: "Resume with from_slot at the lowest incomplete slot, in steps if Solami cuts a deep replay, and drop every re-sent signature.",
  },
  {
    n: "03",
    title: "Patch",
    tone: "text-replay",
    bar: "bg-replay",
    body: "Re-read the slots around Solami's switch back to live, where it drops the start of a slot, on a short second stream.",
  },
  {
    n: "04",
    title: "Verify",
    tone: "text-verified",
    bar: "bg-verified",
    body: "Rebuild the expected set from getTransactionsForAddress, spot-check full blocks, and compare transaction by transaction.",
  },
];

const FINDINGS = [
  {
    stat: "7–8 slots",
    title: "after a replay, Solami drops the start of a slot",
    body: "When a resumed stream switches from replayed history to the live feed, the slot executing at that moment loses its earliest transactions. From inside the stream nothing looks wrong: the slot still completes.",
  },
  {
    stat: "3,000",
    title: "slots of replay history, not 3,500",
    body: "SubscribeReplayInfo reports about 3,000 slots. At today's 264 ms slots that is roughly 13 minutes of outage you can recover, not 23.",
  },
  {
    stat: "EOF",
    title: "is all some backpressure closes send",
    body: "A replay that overflows Solami's 8,192-message buffer can end as a clean end-of-stream. Only the account API's connection history says it was backpressure, about 4–10 s later.",
  },
];

function useProof() {
  const snapshot = useFeed((s) => s.snapshot);
  const incidents = useFeed((s) => s.incidents);
  const verified = incidents.find((i) => i.verification?.report);
  return { snapshot, verified };
}

export function Landing() {
  const link = useFeed((s) => s.link);
  const hasFeed = useFeed((s) => s.snapshot !== null);
  const preview = useMemo(() => createPreviewTape(), []);
  const live = link === "open" && hasFeed;
  const { snapshot, verified } = useProof();
  const report = verified?.verification?.report;

  return (
    <div className="flex min-h-dvh flex-col">
      <header className="border-b border-hairline">
        <div className="mx-auto flex h-14 max-w-6xl items-center gap-4 px-4 lg:px-6">
          <Wordmark />
          <div className="ml-auto flex items-center gap-2">
            <ThemeToggle />
            <Button asChild size="sm">
              <Link to="/live">
                Open the console
                <ArrowRight aria-hidden="true" />
              </Link>
            </Button>
          </div>
        </div>
      </header>

      <main className="flex-1">
        <section className="mx-auto max-w-6xl px-4 pt-16 pb-10 lg:px-6 lg:pt-24">
          <p className="label flex items-center gap-2">
            <span aria-hidden="true" className="size-1.5 rounded-full bg-live" />
            Solami Yellowstone gRPC · Solana mainnet
          </p>
          <h1 className="mt-5 max-w-[15ch] text-4xl leading-[1.05] font-semibold tracking-[-0.035em] text-balance sm:text-5xl lg:text-6xl">
            Prove your Solana stream didn't miss a thing.
          </h1>
          <p className="mt-6 max-w-[62ch] text-base leading-relaxed text-pretty text-muted-foreground lg:text-lg">
            Gapless resumes Solami's gRPC stream exactly where it broke, drops the duplicates a resume produces,
            and checks every recovered slot against RPC. When Solami itself drops transactions on the way back to
            live, Gapless catches them and reads them again.
          </p>
          <div className="mt-8 flex flex-wrap items-center gap-3">
            <Button asChild size="lg" className="h-10 px-4">
              <Link to="/live">
                Open the live console
                <ArrowRight aria-hidden="true" />
              </Link>
            </Button>
            <Button asChild variant="ghost" size="lg" className="h-10 px-3 text-muted-foreground">
              <a href="#loop">
                How it works
                <ArrowDown aria-hidden="true" />
              </a>
            </Button>
          </div>
        </section>

        <section aria-labelledby="tape-title" className="mx-auto max-w-6xl px-4 lg:px-6">
          <div className="rounded-lg border border-hairline bg-panel">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-hairline px-4 py-2.5">
              <div className="flex items-center gap-3">
                <h2 id="tape-title" className="label">
                  Slot tape
                </h2>
                {live ? (
                  <StreamPill />
                ) : (
                  <span className="rounded-full border border-hairline px-2 py-0.5 text-xs text-muted-foreground">
                    Simulated preview · start gapless-server for live data
                  </span>
                )}
              </div>
              {live && snapshot && (
                <dl className="flex items-center gap-4 text-xs">
                  <div className="flex gap-1.5">
                    <dt className="text-faint">Slot</dt>
                    <dd className="num">{fmt.slot(snapshot.metrics.highestComplete)}</dd>
                  </div>
                  <div className="hidden gap-1.5 sm:flex">
                    <dt className="text-faint">Verified to</dt>
                    <dd className="num text-verified">{fmt.slot(snapshot.verifiedThrough)}</dd>
                  </div>
                </dl>
              )}
            </div>
            <SlotTape
              source={live ? liveTape : preview}
              height={132}
              label={
                live
                  ? `Live slot tape. Highest complete slot ${fmt.slot(snapshot?.metrics.highestComplete)}; verified through ${fmt.slot(snapshot?.verifiedThrough)}.`
                  : "Simulated slot tape: live slots, an outage, the replay filling it, and verification following."
              }
            />
            <div className="flex flex-wrap items-center justify-between gap-3 border-t border-hairline px-4 py-2.5">
              <TapeLegend />
              <span className="text-xs text-faint">One bar per slot · height = transactions</span>
            </div>
          </div>

          <dl className="mt-4 grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-hairline bg-hairline md:grid-cols-4">
            {[
              {
                term: "Verified after the last outage",
                value: report ? `${fmt.int(report.matched)} / ${fmt.int(report.expected)}` : "no outage yet",
                tone: report ? "text-verified" : "",
              },
              { term: "Delivered this session", value: fmt.int(snapshot?.metrics.delivered), tone: "" },
              { term: "Duplicates dropped", value: fmt.int(snapshot?.metrics.duplicates), tone: "" },
              {
                term: "Recovered by the handoff patch",
                value: verified?.patch ? fmt.int(verified.patch.recovered) : "no outage yet",
                tone: verified?.patch?.recovered ? "text-replay" : "",
              },
            ].map((item) => (
              <div key={item.term} className="flex flex-col gap-1.5 bg-panel px-4 py-3.5">
                <dt className="text-xs text-muted-foreground">{item.term}</dt>
                <dd className={cn(item.value === "no outage yet" ? "text-sm text-faint" : "num text-lg font-medium", item.tone)}>
                  {item.value}
                </dd>
              </div>
            ))}
          </dl>
        </section>

        <section id="loop" aria-labelledby="loop-title" className="mx-auto max-w-6xl scroll-mt-8 px-4 pt-24 lg:px-6">
          <h2 id="loop-title" className="text-2xl font-semibold tracking-[-0.025em]">
            One outage, four moves
          </h2>
          <p className="mt-2 max-w-[60ch] text-muted-foreground">
            The console breaks the stream on demand, so you can watch each of these happen on mainnet.
          </p>
          <ol className="mt-10 grid gap-8 md:grid-cols-4 md:gap-6">
            {LOOP.map((step) => (
              <li key={step.n} className="relative">
                <div aria-hidden="true" className={cn("h-0.5 w-8 rounded-full", step.bar)} />
                <p className={cn("num mt-4 text-xs", step.tone)}>{step.n}</p>
                <h3 className="mt-1 font-semibold">{step.title}</h3>
                <p className="mt-2 text-sm leading-relaxed text-muted-foreground">{step.body}</p>
              </li>
            ))}
          </ol>
        </section>

        <section aria-labelledby="findings-title" className="mx-auto max-w-6xl px-4 pt-24 pb-24 lg:px-6">
          <h2 id="findings-title" className="text-2xl font-semibold tracking-[-0.025em]">
            What we measured about Solami's stream
          </h2>
          <p className="mt-2 max-w-[60ch] text-muted-foreground">
            From live mainnet runs while building Gapless. Each one changed how it recovers.
          </p>
          <div className="mt-10 divide-y divide-hairline border-y border-hairline">
            {FINDINGS.map((f) => (
              <article key={f.title} className="grid gap-2 py-6 md:grid-cols-[12rem_1fr] md:gap-8">
                <p className="num text-2xl font-medium tracking-tight">{f.stat}</p>
                <div>
                  <h3 className="font-medium">{f.title}</h3>
                  <p className="mt-1.5 max-w-[68ch] text-sm leading-relaxed text-muted-foreground">{f.body}</p>
                </div>
              </article>
            ))}
          </div>
        </section>
      </main>

      <footer className="border-t border-hairline">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-3 px-4 py-6 text-xs text-muted-foreground lg:px-6">
          <span>Built on Solami: Yellowstone gRPC, RPC and the account API.</span>
          <span className="num">MIT</span>
        </div>
      </footer>
    </div>
  );
}
