// Turn the recorder's frames and speed marks into an MP4: real time where it matters, time-lapse
// while waiting, cut where a take didn't happen. Usage: node make-video.mjs <outDir> <file.mp4>
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const [dir, mp4] = process.argv.slice(2);
const { frames, marks } = JSON.parse(readFileSync(`${dir}/timeline.json`, "utf8"));
const speedAt = (t) => {
  let s = null;
  for (const m of marks) if (m.t <= t) s = m.speed;
  return s;
};

const lines = ["ffconcat version 1.0"];
let total = 0;
let last = null;
for (let i = 0; i < frames.length - 1; i++) {
  const speed = speedAt(frames[i].t);
  if (!speed) continue;
  const d = (frames[i + 1].t - frames[i].t) / speed;
  lines.push(`file '${frames[i].file}'`, `duration ${d.toFixed(5)}`);
  total += d;
  last = frames[i].file;
}
lines.push(`file '${last}'`, "duration 1.5", `file '${last}'`);
total += 1.5;
writeFileSync(`${dir}/frames.ffconcat`, lines.join("\n") + "\n");
console.log(`video length ${total.toFixed(1)}s`);

execFileSync(
  "ffmpeg",
  [
    "-y", "-loglevel", "error",
    "-f", "concat", "-safe", "0", "-i", `${dir}/frames.ffconcat`,
    "-vf", `fps=30,format=yuv420p,fade=t=in:st=0:d=0.6,fade=t=out:st=${(total - 0.8).toFixed(2)}:d=0.8`,
    "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-movflags", "+faststart",
    mp4,
  ],
  { stdio: "inherit" },
);
