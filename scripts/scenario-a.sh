#!/usr/bin/env bash
# Scenario A: kill our stream through Solami's account API, stay offline, replay, verify.
# Passes when every expected transaction in the recovered window was delivered.
#   HOLD=30 scripts/scenario-a.sh
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need
HOLD=${HOLD:-30}

say "A: kill → replay → verified (offline ${HOLD}s)"
wait_live 180
post /api/chaos/patch '{"enabled":true}' >/dev/null
before=$(latest_incident)
post /api/chaos/kill "{\"holdSecs\":$HOLD}" | jq -c .
id=$(wait_new_incident "$before" 60)
inc=$(wait_verified "$id" $((HOLD + 420)))
describe <<<"$inc"
verdict=$(jq -r '.verification.report.verdict.kind // "none"' <<<"$inc")
[[ $verdict == complete ]] || fail "verdict is $verdict"
say "PASS A"
