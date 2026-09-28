// Films the Gapless demo against a running live gapless-server: real clicks in headless Chrome,
// captions, a visible cursor, and speed marks for the waits. Writes frames and timeline.json.
//   node record.mjs http://localhost:8790 take && node make-video.mjs take gapless-demo.mp4
// CHROME overrides the browser path.
import { mkdirSync, writeFileSync } from "node:fs";
import puppeteer from "puppeteer-core";

const [base, out] = process.argv.slice(2);
mkdirSync(`${out}/frames`, { recursive: true });
const browser = await puppeteer.launch({
  executablePath:
    process.env.CHROME ??
    (process.platform === "darwin"
      ? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
      : "/usr/bin/google-chrome"),
  headless: true,
  args: ["--hide-scrollbars", "--force-color-profile=srgb"],
  defaultViewport: { width: 1920, height: 1080, deviceScaleFactor: 1 },
});
const page = await browser.newPage();
// Every clock on screen in one zone: the status bar's wall clock is UTC.
await page.emulateTimezone("UTC");
const cdp = await page.createCDPSession();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => Date.now() / 1000;
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);

// ── frames and speed marks ───────────────────────────────────────────────
const frames = [];
const marks = [];
let n = 0;
let recording = false;
let seen = 0;
cdp.on("Page.screencastFrame", ({ data, sessionId }) => {
  cdp.send("Page.screencastFrameAck", { sessionId }).catch(() => {});
  if (!recording) return;
  // A time-lapse only needs every nth frame, and a take that gets cut needs none.
  const speed = marks.at(-1)?.speed ?? 1;
  if (speed === 0 || seen++ % Math.max(1, Math.round(speed)) !== 0) return;
  const file = `frames/${String(n++).padStart(6, "0")}.jpg`;
  writeFileSync(`${out}/${file}`, Buffer.from(data, "base64"));
  frames.push({ file, t: now() });
});
const mark = (label, speed) => {
  marks.push({ label, speed, t: now() });
  log("mark", label, speed);
};

// ── overlay: caption, time-lapse badge, cursor ───────────────────────────
async function overlay() {
  await page.evaluate(() => {
    if (document.getElementById("demo-caption")) return;
    const style = document.createElement("style");
    style.textContent = `
      #demo-caption { position: fixed; left: 50%; bottom: 56px; transform: translateX(-50%); width: max-content; max-width: 1040px;
        padding: 14px 26px; border-radius: 14px; background: rgba(14,16,21,.92); color: #eef1f5;
        font: 500 25px/1.38 "Geist Variable", system-ui, sans-serif; letter-spacing: -0.012em; text-align: center; text-wrap: balance;
        border: 1px solid rgba(255,255,255,.14); box-shadow: 0 10px 40px rgba(0,0,0,.45); z-index: 2147483000; transition: opacity .35s; }
      #demo-caption:empty { opacity: 0; }
      #demo-badge { position: fixed; top: 72px; left: 50%; transform: translateX(-50%); padding: 6px 14px; border-radius: 999px;
        background: rgba(240,190,90,.16); color: #f3c878; border: 1px solid rgba(243,200,120,.45);
        font: 600 15px/1 "Geist Mono Variable", ui-monospace, monospace; letter-spacing: .08em; z-index: 2147483000; transition: opacity .3s; }
      #demo-badge:empty { opacity: 0; }
      #demo-cursor { position: fixed; left: 0; top: 0; z-index: 2147483001; pointer-events: none;
        transform: translate(960px, 1200px); transition: transform .75s cubic-bezier(.2,.8,.2,1); }
      #demo-cursor svg { transition: transform .12s; transform-origin: 4px 3px; filter: drop-shadow(0 2px 4px rgba(0,0,0,.5)); }
      #demo-cursor.press svg { transform: scale(.82); }
    `;
    document.head.appendChild(style);
    const caption = Object.assign(document.createElement("div"), { id: "demo-caption" });
    const badge = Object.assign(document.createElement("div"), { id: "demo-badge" });
    const cursor = Object.assign(document.createElement("div"), { id: "demo-cursor" });
    cursor.innerHTML = `<svg width="30" height="30" viewBox="0 0 24 24"><path d="M4 3l7.2 17.2 2.3-7.1 7.1-2.3z" fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/></svg>`;
    document.body.append(caption, badge, cursor);
  });
}
const caption = (text) => page.evaluate((t) => (document.getElementById("demo-caption").textContent = t), text);
const badge = (text) => page.evaluate((t) => (document.getElementById("demo-badge").textContent = t), text ?? "");

async function center(handle) {
  const box = await handle.boundingBox();
  if (!box) throw new Error("element not visible");
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}
async function moveTo(handle, pause = 850) {
  const { x, y } = await center(handle);
  await page.evaluate((x, y) => (document.getElementById("demo-cursor").style.transform = `translate(${x - 4}px, ${y - 3}px)`), x, y);
  await sleep(pause);
  return { x, y };
}
async function click(handle) {
  const { x, y } = await moveTo(handle);
  await page.evaluate(() => document.getElementById("demo-cursor").classList.add("press"));
  await page.mouse.click(x, y);
  await sleep(140);
  await page.evaluate(() => document.getElementById("demo-cursor").classList.remove("press"));
  await sleep(350);
}
const byText = async (selector, text) => {
  const h = await page.evaluateHandle(
    (s, t) => [...document.querySelectorAll(s)].find((e) => e.textContent.trim() === t) ?? null,
    selector,
    text,
  );
  if (!h.asElement()) throw new Error(`no ${selector} "${text}"`);
  return h.asElement();
};
const api = (path) => page.evaluate(async (p) => (await fetch(p)).json(), path);
const latest = async () => (await api("/api/incidents?limit=1"))[0] ?? null;
async function until(check, timeoutMs, everyMs = 500) {
  const end = Date.now() + timeoutMs;
  for (;;) {
    const v = await check();
    if (v) return v;
    if (Date.now() > end) throw new Error("timed out");
    await sleep(everyMs);
  }
}

// ── one outage, filmed ───────────────────────────────────────────────────
// Only break a stream that's caught up: filming on an overloaded machine makes a lagging take.
async function settled() {
  await until(async () => {
    const s = await api("/api/state");
    const lag = (s.tip ?? 0) - (s.metrics.highestComplete ?? 0);
    return s.state.kind === "live" && s.controls.canKill && !s.openIncident && lag <= 10;
  }, 180_000, 1000);
}

async function outage(holdLabel, words) {
  await settled();
  await click(await byText("label", holdLabel));
  const before = (await latest())?.id ?? 0;
  await click(await byText("button", "Kill"));
  await sleep(500);
  await click(await byText("button", "Confirm kill"));
  const id = await until(async () => {
    const i = await latest();
    return i && i.id > before ? i.id : null;
  }, 20_000);
  await sleep(2500);
  mark(`offline #${id}`, 3);
  await badge("TIME-LAPSE 3×");
  await caption(words.offline);
  await until(async () => {
    const s = await api("/api/state");
    return s.state.kind === "replaying" || s.state.kind === "live";
  }, 180_000);
  mark(`replay #${id}`, 1);
  await badge(null);
  await caption(words.replay);
  await until(async () => (await api(`/api/incidents/${id}`)).recoveredAt, 120_000);
  await sleep(2500);
  mark(`finalize #${id}`, 4);
  await badge("TIME-LAPSE 4×");
  await caption(words.finalize);
  const done = await until(async () => {
    const i = await api(`/api/incidents/${id}`);
    return ["done", "failed"].includes(i.verification?.status) ? i : null;
  }, 300_000, 1000);
  await sleep(1200);
  await badge(null);
  return done;
}

// ── the script ───────────────────────────────────────────────────────────
await page.goto(base + "/", { waitUntil: "networkidle2" });
await page.evaluate(() => localStorage.setItem("gapless-theme", "dark"));
const state0 = await api("/api/state");
if (!state0.controls.handoffPatch) await page.evaluate(() => fetch("/api/chaos/patch", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ enabled: true }) }));
await page.goto(base + "/", { waitUntil: "networkidle2" });
await overlay();
await sleep(2500);
await cdp.send("Page.startScreencast", { format: "jpeg", quality: 82, maxWidth: 1920, maxHeight: 1080, everyNthFrame: 3 });
recording = true;

mark("problem", 1);
await caption("Your bot reconnects. Did it miss anything? Did it count something twice?");
await sleep(5000);
await caption("Right now, you guess. Gapless proves it, live on Solana mainnet through Solami.");
await sleep(4500);
await click(await byText("a", "Open the live console"));
await sleep(1800);

mark("live", 1);
await caption("Pump.fun transactions, streaming live through Solami's Yellowstone gRPC.");
await sleep(4500);
await moveTo(await page.$("canvas[role=img]"));
await caption("One bar per slot. Green arrived live; blue is already verified against Solami's RPC.");
await sleep(5500);
await moveTo(await byText("dt", "Solami"));
await caption("Our stream's region and send buffer, read from Solami's own account API.");
await sleep(4500);

mark("break", 1);
await caption("Break it: kill our stream through Solami's account API, and stay offline for 30 seconds.");
const first = await outage("30s", {
  offline: "Offline. The chain keeps going, so the gap grows on the tape.",
  replay: "Back online: resume with from_slot at the exact slot where it broke. The replay (amber) catches up to live.",
  finalize: "The handoff patch re-reads the switch to live, then Gapless waits for the slots to finalize.",
});
mark("verified", 1);
{
  const r = first.verification.report;
  await moveTo(await page.$("ol li[aria-label^='Verified']"));
  await caption(
    `Checked against Solami's getTransactionsForAddress: ${r.matched.toLocaleString("en-US")} of ${r.expected.toLocaleString("en-US")} delivered. ${first.duplicates} duplicates dropped, nothing counted twice.`,
  );
  await sleep(7000);
}

// The twist, retried (and cut) until Solami actually drops something at the handoff.
let twist = null;
for (let attempt = 1; attempt <= 5 && !twist; attempt++) {
  const setup = marks.length;
  mark(`twist ${attempt}`, 1);
  await caption("The twist: turn the handoff patch off, and break it again.");
  const sw = await page.$("#handoff-patch");
  if ((await sw.evaluate((e) => e.getAttribute("aria-checked"))) === "true") await click(sw);
  await sleep(700);
  const inc = await outage("15s", {
    offline: "Offline for 15 seconds, patch off.",
    replay: "Replayed. From inside the stream, everything looks complete.",
    finalize: "Verifying against RPC…",
  });
  const missing = inc.verification.report?.missing.length ?? 0;
  if (missing > 0) {
    twist = inc;
  } else {
    log(`attempt ${attempt}: clean handoff, cutting it`);
    for (const m of marks.slice(setup)) m.speed = 0;
    mark(`after cut ${attempt}`, 0);
  }
}
if (twist) {
  const r = twist.verification.report;
  mark("twist result", 1);
  await moveTo(await page.$("ol li[aria-label^='Verified']"));
  await caption(`Solami dropped ${r.missing.length} transactions at the switch back to live. The stream never showed a problem; only verification caught them.`);
  await sleep(6500);
  await click(await byText("button", "Details"));
  await sleep(1500);
  await page.evaluate(() => {
    const scroller = document.querySelector("[data-slot=sheet-content] .overflow-y-auto");
    scroller?.scrollTo({ top: scroller.scrollHeight, behavior: "smooth" });
  });
  await caption("Each one found in its block, fetched back from RPC, and linked on Solscan. This is why you verify.");
  await sleep(900);
  await moveTo(await page.$("[data-slot=sheet-content] a[href^='https://solscan.io/tx/']"));
  await sleep(6000);
  await page.keyboard.press("Escape");
  await sleep(1000);

  mark("patch on", 1);
  await caption("Patch back on. Same outage.");
  await click(await page.$("#handoff-patch"));
  await sleep(600);
  const on = await outage("15s", {
    offline: "Offline for 15 seconds, patch on.",
    replay: "Replaying…",
    finalize: "The patch re-reads the handoff slots. Verifying…",
  });
  const r2 = on.verification.report;
  mark("patch on result", 1);
  await moveTo(await page.$("ol li[aria-label^='Verified']"));
  await caption(
    on.patch?.recovered
      ? `The patch recovered ${on.patch.recovered} transactions Solami dropped, and the window verifies complete: ${r2.matched.toLocaleString("en-US")} of ${r2.expected.toLocaleString("en-US")}.`
      : `Complete: ${r2.matched.toLocaleString("en-US")} of ${r2.expected.toLocaleString("en-US")}.`,
  );
  await sleep(7000);
}

mark("history", 1);
await caption("Every outage is kept: cause, gap, replay, patch and verification.");
await click(await page.$("header nav a[href='/incidents']"));
await sleep(5500);

mark("use it", 1);
await click(await page.$("header nav a[href='/integrate']"));
await caption("Use it: put Gapless between Solami and your bot. Each transaction exactly once, through any outage.");
await sleep(1200);
await moveTo(await page.$("pre"));
await sleep(5000);
await caption("Open source (MIT): github.com/vignesh-chaturvedi/gapless. docker compose up runs it with or without a key.");
await sleep(6500);
mark("end", 0);

recording = false;
await cdp.send("Page.stopScreencast");
writeFileSync(`${out}/timeline.json`, JSON.stringify({ frames, marks, twist: Boolean(twist) }, null, 1));
log("frames", frames.length, "twist", Boolean(twist));
await browser.close();
