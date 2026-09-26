// Types for gapless-server's REST and WebSocket API. Keep in sync with docs/api.md.

export interface SlotRange {
  first: number;
  last: number;
}

export type StreamState =
  | { kind: "connecting"; attempt: number }
  | { kind: "replaying"; incident: number; fromSlot: number; targetSlot: number }
  | { kind: "live" }
  | { kind: "backoff"; delayMs: number; attempt: number }
  | { kind: "stopped"; reason: string };

export interface Metrics {
  updatesPerSec: number;
  txPerSec: number;
  highestComplete: number | null;
  tip: number | null;
  lagSlots: number | null;
  delivered: number;
  duplicates: number;
  reconnects: number;
  incidents: number;
  latencyMs: number | null;
  dedupEntries: number;
}

export interface SolamiConn {
  connId: string;
  region: string | null;
  bytesStreamed: number;
  throughputBps: number;
  bufferSize: number;
  bufferPending: number;
  isPaygo: boolean;
  liveStreams: number;
  sampledAt: number;
}

export interface Controls {
  handoffPatch: boolean;
  throttleMs: number | null;
  holdSecs: number | null;
  canKill: boolean;
}

export interface Minute {
  minute: number;
  txs: number;
  trades: number;
  buys: number;
  sells: number;
  buySol: number;
  sellSol: number;
  traders: number;
  launches: number;
  graduations: number;
}

export interface Launch {
  mint: string;
  name: string;
  symbol: string;
  at: number;
}

export interface Indexer {
  txs: number;
  trades: number;
  buys: number;
  sells: number;
  buySol: number;
  sellSol: number;
  uniqueTraders: number;
  launches: number;
  graduations: number;
  minutes: Minute[];
  recentLaunches: Launch[];
}

export interface Snapshot {
  mode: "live" | "offline";
  program: string;
  startedAt: number;
  state: StreamState;
  connId: string | null;
  metrics: Metrics;
  tip: number | null;
  solami: SolamiConn | null;
  verifiedThrough: number | null;
  controls: Controls;
  indexer: Indexer;
  openIncident: Incident | null;
}

export interface SlotCell {
  slot: number;
  origin: "live" | "replay";
  complete: boolean;
  txs: number;
  verified: "ok" | "missing" | "repaired" | null;
  missing: number;
  incident: number | null;
  at: number;
}

export type TxKind = "buy" | "sell" | "create" | "complete" | "other";

export interface Tx {
  sig: string;
  slot: number;
  origin: "live" | "replay" | "patch";
  kind: TxKind;
  sol: number | null;
  mint: string | null;
  symbol: string | null;
  user: string | null;
  at: number;
}

export interface LogLine {
  at: number;
  level: "info" | "warn" | "error" | "success";
  text: string;
  incident: number | null;
}

export interface Reason {
  code: string;
  text: string;
}

export interface Step {
  attempt: number;
  fromSlot: number;
  targetSlot: number;
  startedAt: number;
  ended: Reason | null;
  endedDetail: string | null;
  transactions: number;
}

export interface MissingTx {
  signature: string;
  slot: number;
  position: number | null;
  cause: string | null;
  incident: number | null;
}

export interface SpotCheck {
  slot: number;
  hasBlock: boolean;
  fromBlock: number;
  fromHistory: number;
  viaLookupTable: number;
  agree: boolean;
  onlyInBlock: string[];
  onlyInHistory: string[];
}

export type Verdict =
  | { kind: "complete" }
  | { kind: "incomplete"; missing: number }
  | { kind: "repaired"; repaired: number }
  | { kind: "inconclusive"; reason: string };

export interface VerificationReport {
  range: SlotRange;
  finalizedTip: number;
  source: string;
  addresses: string[];
  slotsWithBlocks: number;
  skippedSlots: number;
  expected: number;
  delivered: number;
  matched: number;
  missing: MissingTx[];
  repaired: { signature: string; slot: number; blockTime: number | null; feePayer: string | null }[];
  orphaned: { signature: string; slot: number }[];
  landedElsewhere: { signature: string; deliveredSlot: number; landedSlot: number }[];
  unexplained: { signature: string; slot: number }[];
  spotChecks: SpotCheck[];
  verdict: Verdict;
  perSlot: { slot: number; hasBlock: boolean; expected: number; delivered: number; matched: number }[];
  rpc: { calls: number; bytes: number; retries: number };
  elapsedMs: number;
}

export interface Verification {
  status: "pending" | "running" | "done" | "failed" | "skipped";
  range: SlotRange | null;
  error: string | null;
  report: VerificationReport | null;
}

export interface Incident {
  id: number;
  sessionIncident: number;
  status: "open" | "recovered" | "verified";
  reason: Reason;
  detail: string;
  openedAt: number;
  recoveredAt: number | null;
  durationMs: number | null;
  lastCompleteSlot: number | null;
  resumeFrom: number | null;
  gap: SlotRange | null;
  unrecoverable: SlotRange | null;
  steps: Step[];
  replayed: number;
  duplicates: number;
  naiveDoubleCounts: number;
  patch: { slots: SlotRange; recovered: number; error: string | null } | null;
  verification: Verification | null;
  chaos: string | null;
}

export type WsMessage =
  | { type: "hello"; snapshot: Snapshot; tape: SlotCell[]; incidents: Incident[]; txs: Tx[]; log: LogLine[] }
  | { type: "batch"; slots: SlotCell[]; txs: Tx[]; txCount: number; log: LogLine[] }
  | { type: "tick"; snapshot: Snapshot }
  | { type: "incident"; incident: Incident }
  | { type: "verified"; range: SlotRange; slots: SlotCell[] };
