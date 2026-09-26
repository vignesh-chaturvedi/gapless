#!/usr/bin/env bash
# Scenario B: make the consumer slow until Solami's buffer fills and it drops the stream for
# backpressure (the default `disconnect` policy). Gapless notices the close through the account
# API, restores full speed, replays and verifies.
#   scripts/scenario-b.sh
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need
PER_UPDATE_MS=${PER_UPDATE_MS:-250}
trap 'post /api/chaos/slow "{\"perUpdateMs\":null}" >/dev/null 2>&1 || true' EXIT

say "B: slow consumer (${PER_UPDATE_MS} ms per update) → backpressure → recovery"
wait_live 180
post /api/chaos/patch '{"enabled":true}' >/dev/null
before=$(latest_incident)
post /api/chaos/slow "{\"perUpdateMs\":$PER_UPDATE_MS}" | jq -c .
# Solami's 8,192-message buffer takes a couple of minutes to fill at Pump.fun's rate.
deadline=$((SECONDS + 420))
while ((SECONDS < deadline)); do
  s=$(api /api/state)
  if (($(latest_incident) > before)); then break; fi
  say "buffer $(jq -r '.solami.bufferPending // "--"' <<<"$s") / $(jq -r '.solami.bufferSize // "--"' <<<"$s"), lag $(jq -r '(.tip // 0) - (.metrics.highestComplete // 0)' <<<"$s") slots"
  sleep 10
done
id=$(wait_new_incident "$before" 10)
[[ $(api /api/state | jq -r .controls.throttleMs) == null ]] || fail "the slow consumer wasn't released"
inc=$(wait_verified "$id" 600)
describe <<<"$inc"
reason=$(jq -r .reason.code <<<"$inc")
verdict=$(jq -r '.verification.report.verdict.kind // "none"' <<<"$inc")
[[ $reason == backpressure ]] || fail "reason is $reason, not backpressure"
[[ $verdict == complete ]] || fail "verdict is $verdict"
say "PASS B"
