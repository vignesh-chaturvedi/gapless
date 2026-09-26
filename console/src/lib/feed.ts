import { create } from "zustand";

import type { Incident, LogLine, SlotCell, Snapshot, Tx, WsMessage } from "@/lib/types";

const TAPE_KEEP = 1_500;
const TXS_KEEP = 200;
const LOG_KEEP = 150;

/** The WebSocket to gapless-server, not the Solami stream. */
export type Link = "connecting" | "open" | "closed";

interface FeedState {
  link: Link;
  /** When the link last closed, for "reconnecting in…" copy. */
  closedAt: number | null;
  snapshot: Snapshot | null;
  /** Slot → cell. Replaced (not mutated) on every change so selectors notice. */
  tape: Map<number, SlotCell>;
  txs: Tx[];
  log: LogLine[];
  incidents: Incident[];
  lastMessageAt: number | null;
}

export const useFeed = create<FeedState>(() => ({
  link: "connecting",
  closedAt: null,
  snapshot: null,
  tape: new Map(),
  txs: [],
  log: [],
  incidents: [],
  lastMessageAt: null,
}));

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
        txs: [...message.txs].reverse().slice(0, TXS_KEEP),
        log: [...message.log].reverse().slice(0, LOG_KEEP),
        lastMessageAt: now,
      });
      break;
    case "batch":
      useFeed.setState((s) => ({
        tape: mergeCells(s.tape, message.slots),
        txs: message.txs.length ? [...[...message.txs].reverse(), ...s.txs].slice(0, TXS_KEEP) : s.txs,
        log: message.log.length ? [...[...message.log].reverse(), ...s.log].slice(0, LOG_KEEP) : s.log,
        lastMessageAt: now,
      }));
      break;
    case "tick":
      useFeed.setState({ snapshot: message.snapshot, lastMessageAt: now });
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
