#!/usr/bin/env bash
# Sample a running gapless-server once a minute for MINUTES (default 120): memory, stream state,
# counters and in-memory sizes, into a CSV, then summarise.
#   MINUTES=120 scripts/soak.sh soak.csv
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need
out=${1:-soak.csv}
MINUTES=${MINUTES:-120}
port=${GAPLESS_URL##*:}
pid=${PID:-$(lsof -ti "tcp:$port" -sTCP:LISTEN | head -1)}
[[ -n $pid ]] || fail "can't find the server process on port $port"

echo "at,rss_mb,state,lag,tx_per_sec,delivered,duplicates,reconnects,incidents,dedup,delivered_map,ledger,tracked,verified_behind" >"$out"
for ((i = 0; i <= MINUTES; i++)); do
  s=$(api /api/state) h=$(api /api/health)
  rss=$(ps -o rss= -p "$pid" | awk '{printf "%.1f", $1/1024}')
  row=$(jq -r --arg rss "$rss" --argjson h "$h" '[
    (now | strftime("%H:%M:%S")), $rss, .state.kind,
    ((.tip // 0) - (.metrics.highestComplete // 0)), (.metrics.txPerSec | floor),
    .metrics.delivered, .metrics.duplicates, .metrics.reconnects, .metrics.incidents,
    $h.sizes.dedup, $h.sizes.delivered, $h.sizes.ledger, $h.sizes.trackedIncidents,
    ((.metrics.highestComplete // 0) - (.verifiedThrough // 0))
  ] | @csv' <<<"$s")
  echo "$row" >>"$out"
  say "$row"
  ((i < MINUTES)) && sleep 60
done
awk -F, 'NR>1{gsub(/"/,""); if (NR==2) first=$2; if ($2+0>max) max=$2+0; last=$2; r=$8} END{printf "RSS first %s MB, last %s MB, peak %s MB; reconnects %s\n", first, last, max, r}' "$out"
