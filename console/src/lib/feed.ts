import { create } from "zustand";

import type { Incident, LogLine, SlotCell, Snapshot, Tx, WsMessage } from "@/lib/types";

const TAPE_KEEP = 1_500;
const TXS_KEEP = 1_000;
/** Recovered and dropped transactions are rare, so they're kept longer than the main feed. */
const MARKED_KEEP = 400;
const BUFFER_KEEP = 180;
const LOG_KEEP = 150;

/** The WebSocket to gapless-server, not the Solami stream. */
export type Link = "connecting" | "open" | "closed";

/** One reading of Solami's send buffer for our stream. */
export interface BufferSample {
  at: number;
  pending: number;
}

interface FeedState {
  link: Link;
  /** When the link last closed, for "reconnecting in…" copy. */
  closedAt: number | null;
  snapshot: Snapshot | null;
  /** Slot → cell. Replaced (not mutated) on every change so selectors notice. */
  tape: Map<number, SlotCell>;
  txs: Tx[];
  /** Replayed or patched transactions, newest first, kept past the main feed's window. */
  recovered: Tx[];
  /** Duplicates Gapless dropped, newest first. */
  duplicates: Tx[];
  log: LogLine[];
  incidents: Incident[];
  /** Solami's buffer for our stream, one sample per tick, oldest first. */
  buffer: BufferSample[];
  /** Measured milliseconds per slot over the last minute, from the chain tip. */
  slotMs: number | null;
  lastMessageAt: number | null;
}

export const useFeed = create<FeedState>(() => ({
  link: "connecting",
  closedAt: null,
  snapshot: null,
  tape: new Map(),
  txs: [],
  recovered: [],
  duplicates: [],
  log: [],
  incidents: [],
  buffer: [],
  slotMs: null,
  lastMessageAt: null,
}));

const tips: { at: number; tip: number }[] = [];

/** Slot time from how fast the tip moved over the last minute. */
function measureSlotMs(snapshot: Snapshot, now: number): number | null {
  if (snapshot.tip != null) tips.push({ at: now, tip: snapshot.tip });
  while (tips.length && now - tips[0].at > 60_000) tips.shift();
  if (tips.length < 2) return null;
  const first = tips[0];
  const last = tips[tips.length - 1];
  const slots = last.tip - first.tip;
  return slots > 10 ? (last.at - first.at) / slots : null;
}

function sampleBuffer(buffer: BufferSample[], snapshot: Snapshot): BufferSample[] {
  const solami = snapshot.solami;
  if (!solami) return buffer;
  const at = solami.sampledAt;
  if (buffer.length && buffer[buffer.length - 1].at >= at) return buffer;
  const next = [...buffer, { at, pending: solami.bufferPending }];
  return next.length > BUFFER_KEEP ? next.slice(next.length - BUFFER_KEEP) : next;
}

function mergeCells(tape: Map<number, SlotCell>, cells: SlotCell[]): Map<number, SlotCell> {
  if (cells.length === 0) return tape;
  const next = new Map(tape);
  for (const cell of cells) next.set(cell.slot, cell);
  if (next.size > TAPE_KEEP) {
    const drop = [...next.keys()].sort((a, b) => a - b).slice(0, next.size - TAPE_KEEP);
    for (const slot of drop) next.delete(slot);
  }
  return next;
}

/** Newest-first rows prepended to a capped list. `txs` arrive oldest first. */
function prepend(list: Tx[], txs: Tx[], keep: number): Tx[] {
  return txs.length ? [...[...txs].reverse(), ...list].slice(0, keep) : list;
}

const isRecovered = (tx: Tx) => tx.origin === "replay" || tx.origin === "patch";
const isDuplicate = (tx: Tx) => tx.origin === "duplicate";

function upsertIncident(list: Incident[], incident: Incident): Incident[] {
  const rest = list.filter((i) => i.id !== incident.id);
  return [incident, ...rest].sort((a, b) => b.id - a.id).slice(0, 50);
}

function apply(message: WsMessage) {
  const now = Date.now();
  switch (message.type) {
    case "hello":
      useFeed.setState({
        snapshot: message.snapshot,
        tape: mergeCells(new Map(), message.tape),
        incidents: [...message.incidents].sort((a, b) => b.id - a.id),
        txs: prepend([], message.txs, TXS_KEEP),
        recovered: prepend([], message.txs.filter(isRecovered), MARKED_KEEP),
        duplicates: prepend([], message.txs.filter(isDuplicate), MARKED_KEEP),
        log: [...message.log].reverse().slice(0, LOG_KEEP),
        lastMessageAt: now,
      });
      break;
    case "batch":
      useFeed.setState((s) => ({
        tape: mergeCells(s.tape, message.slots),
        txs: prepend(s.txs, message.txs, TXS_KEEP),
        recovered: prepend(s.recovered, message.txs.filter(isRecovered), MARKED_KEEP),
        duplicates: prepend(s.duplicates, message.txs.filter(isDuplicate), MARKED_KEEP),
        log: message.log.length ? [...[...message.log].reverse(), ...s.log].slice(0, LOG_KEEP) : s.log,
        lastMessageAt: now,
      }));
      break;
    case "tick":
      useFeed.setState((s) => ({
        snapshot: message.snapshot,
        buffer: sampleBuffer(s.buffer, message.snapshot),
        slotMs: measureSlotMs(message.snapshot, now) ?? s.slotMs,
        lastMessageAt: now,
      }));
      break;
    case "incident":
      useFeed.setState((s) => ({ incidents: upsertIncident(s.incidents, message.incident), lastMessageAt: now }));
      break;
    case "verified":
      useFeed.setState((s) => ({ tape: mergeCells(s.tape, message.slots), lastMessageAt: now }));
      break;
  }
}

let socket: WebSocket | null = null;
let attempt = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

/** Connect to gapless-server's feed and keep reconnecting with backoff. Idempotent. */
export function connectFeed() {
  if (socket && socket.readyState <= WebSocket.OPEN) return;
  clearTimeout(timer);
  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const ws = new WebSocket(`${scheme}://${location.host}/ws`);
  socket = ws;
  useFeed.setState({ link: "connecting" });
  ws.onopen = () => {
    attempt = 0;
    useFeed.setState({ link: "open", closedAt: null });
  };
  ws.onmessage = (event) => {
    try {
      apply(JSON.parse(event.data as string) as WsMessage);
    } catch (error) {
      console.warn("gapless feed: bad message", error);
    }
  };
  ws.onclose = () => {
    if (socket !== ws) return;
    socket = null;
    useFeed.setState((s) => ({ link: "closed", closedAt: s.closedAt ?? Date.now() }));
    const delay = Math.min(10_000, 500 * 2 ** attempt) * (0.75 + Math.random() * 0.5);
    attempt += 1;
    timer = setTimeout(connectFeed, delay);
  };
}

/** Reconnect now (from a "Retry" button). */
export function reconnectFeed() {
  attempt = 0;
  socket?.close();
  socket = null;
  connectFeed();
}
