import { Check, Copy } from "lucide-react";
import { useState } from "react";

import { Panel } from "@/components/panel";
import { Button } from "@/components/ui/button";

const RUST = `use futures::StreamExt;
use gapless::{Event, Gapless};

let (mut events, _control) = Gapless::builder(std::env::var("SOLAMI_API_KEY")?)
    .program("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
    .build()?
    .start();

while let Some(event) = events.next().await {
    match event {
        // Each transaction exactly once, across every disconnect.
        Event::Transaction(tx) => index(&tx),
        // An outage closed: gap, replay steps, duplicates dropped.
        Event::Recovered(incident) => log(&incident),
        _ => {}
    }
}`;

const REPO = "https://github.com/vignesh-chaturvedi/gapless";

const GET = [
  { comment: "Clone and run the console (no key: recorded mainnet; add SOLAMI_API_KEY to .env for live)", cmd: `git clone ${REPO} && cd gapless && docker compose up` },
  { comment: "Add the library to your Cargo.toml", cmd: `gapless = { git = "${REPO}" }` },
];

const COMMANDS = [
  { comment: "Watch a stream recover (kill it from Solami's dashboard while it runs)", cmd: "GAPLESS_HOLD_SECS=60 cargo run -p gapless --example tail --release" },
  { comment: "Stream, kill, replay, then verify everything against RPC", cmd: "cargo run -p gapless-verify --release -- run --secs 120 --kill-after 20 --hold 60" },
  { comment: "This console, live", cmd: "cargo run -p gapless-server --release" },
  { comment: "This console, no API key: recorded mainnet", cmd: "cargo run -p gapless-server --release -- --offline fixtures/pumpfun-150s.bin.zst" },
];

const ENV = [
  { name: "SOLAMI_API_KEY", note: "Standard key with the Developer role (StreamsKill, ConnectionsView)" },
  { name: "GAPLESS_PROGRAM", note: "Program to follow. Default: Pump.fun" },
  { name: "SOLAMI_GRPC_URL", note: "Default https://grpc.solami.dev" },
  { name: "SOLAMI_RPC_URL", note: "Default https://rpc.solami.dev/sol" },
  { name: "SOLAMI_API_URL", note: "Default https://api.solami.dev" },
];

function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={copied ? "Copied" : label}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        } catch {
          // Clipboard can be blocked; the text is selectable either way.
        }
      }}
    >
      {copied ? <Check aria-hidden="true" className="text-live" /> : <Copy aria-hidden="true" />}
    </Button>
  );
}

export function Integrate() {
  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-4 px-4 py-6 lg:px-6">
      <div>
        <h1 className="text-xl font-semibold tracking-[-0.02em]">Integrate</h1>
        <p className="mt-1 max-w-[70ch] text-sm text-muted-foreground">
          The console is built on the <span className="num text-foreground">gapless</span> crate. Put it between Solami
          and your own bot or indexer and you get each transaction once, even across disconnects.
        </p>
      </div>

      <Panel
        title="Get it"
        aside={
          <a
            href={REPO}
            target="_blank"
            rel="noreferrer"
            className="num rounded-sm text-foreground underline-offset-2 hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            github.com/vignesh-chaturvedi/gapless
          </a>
        }
        bodyClassName="divide-y divide-hairline"
      >
        {GET.map((c) => (
          <div key={c.cmd} className="flex items-start justify-between gap-4 px-4 py-3">
            <div className="min-w-0">
              <p className="text-sm text-muted-foreground">{c.comment}</p>
              <code className="num mt-1 block overflow-x-auto text-[13px] whitespace-nowrap">{c.cmd}</code>
            </div>
            <CopyButton text={c.cmd} label={`Copy: ${c.cmd}`} />
          </div>
        ))}
      </Panel>

      <Panel title="In your code" aside={<CopyButton text={RUST} label="Copy the Rust example" />}>
        <pre className="num overflow-x-auto px-4 py-4 text-[13px] leading-relaxed">
          <code>{RUST}</code>
        </pre>
      </Panel>

      <Panel title="Run it" bodyClassName="divide-y divide-hairline">
        {COMMANDS.map((c) => (
          <div key={c.cmd} className="flex items-start justify-between gap-4 px-4 py-3">
            <div className="min-w-0">
              <p className="text-sm text-muted-foreground">{c.comment}</p>
              <code className="num mt-1 block overflow-x-auto text-[13px] whitespace-nowrap">{c.cmd}</code>
            </div>
            <CopyButton text={c.cmd} label={`Copy: ${c.cmd}`} />
          </div>
        ))}
      </Panel>

      <Panel title="Environment" bodyClassName="divide-y divide-hairline">
        {ENV.map((e) => (
          <div key={e.name} className="grid gap-1 px-4 py-3 sm:grid-cols-[14rem_1fr] sm:gap-4">
            <code className="num text-[13px]">{e.name}</code>
            <span className="text-sm text-muted-foreground">{e.note}</span>
          </div>
        ))}
      </Panel>
    </div>
  );
}
