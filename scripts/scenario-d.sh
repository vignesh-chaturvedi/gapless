#!/usr/bin/env bash
# Scenario D: stay offline longer than Solami's replay horizon (3,000 slots, ~13 min). Gapless
# replays what's still available and reports the rest as lost, with the exact slot range; the
# recovered part still verifies complete.
#   scripts/scenario-d.sh                 # live: offline 15 minutes
#   HOLD=90 scripts/scenario-d.sh         # against `gapless-server --offline ... --horizon 200`
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need
HOLD=${HOLD:-900}

say "D: outage beyond the replay horizon (offline ${HOLD}s)"
wait_live 240
post /api/chaos/patch '{"enabled":true}' >/dev/null
before=$(latest_incident)
post /api/chaos/kill "{\"holdSecs\":$HOLD}" | jq -c .
id=$(wait_new_incident "$before" 60)
inc=$(wait_verified "$id" $((HOLD + 900)))
describe <<<"$inc"
[[ $(jq -r '.unrecoverable != null' <<<"$inc") == true ]] || fail "no slots were reported lost; was the outage long enough?"
verdict=$(jq -r '.verification.report.verdict.kind // "none"' <<<"$inc")
[[ $verdict == complete ]] || fail "the recovered part verified $verdict"
say "PASS D: lost $(jq -r '"\(.unrecoverable.last - .unrecoverable.first + 1) slots (\(.unrecoverable.first)–\(.unrecoverable.last))"' <<<"$inc"), the rest verified complete"
