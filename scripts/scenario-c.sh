#!/usr/bin/env bash
# Scenario C, the twist: Solami drops transactions at the replay-to-live handoff, and nothing in
# the stream shows it. With the handoff patch off, verification catches the missing ones (and
# fetches them from RPC). With it on, the same outage verifies complete.
#   HOLD=30 scripts/scenario-c.sh
set -euo pipefail
source "$(dirname "$0")/lib.sh"
need
HOLD=${HOLD:-30}
trap 'post /api/chaos/patch "{\"enabled\":true}" >/dev/null 2>&1 || true' EXIT

outage() {
  wait_live 240
  local before; before=$(latest_incident)
  post /api/chaos/kill "{\"holdSecs\":$HOLD}" >/dev/null
  local id; id=$(wait_new_incident "$before" 60)
  wait_verified "$id" $((HOLD + 420))
}

say "C1: handoff patch OFF, kill, offline ${HOLD}s"
post /api/chaos/patch '{"enabled":false}' >/dev/null
off=$(outage)
describe <<<"$off"
missing=$(jq '.verification.report.missing | length' <<<"$off")
at_handoff=$(jq '[.verification.report.missing[] | select(.cause == "replay_handoff")] | length' <<<"$off")
repaired=$(jq '.verification.report.repaired | length' <<<"$off")

say "C2: handoff patch ON, same outage"
post /api/chaos/patch '{"enabled":true}' >/dev/null
on=$(outage)
describe <<<"$on"
verdict=$(jq -r '.verification.report.verdict.kind // "none"' <<<"$on")

[[ $verdict == complete ]] || fail "with the patch on, the verdict is $verdict"
if ((missing == 0)); then
  say "INCONCLUSIVE C: Solami dropped nothing at this handoff, so there was nothing to catch. Run it again."
  exit 2
fi
((at_handoff == missing)) || fail "$((missing - at_handoff)) missing transactions weren't at the handoff"
((repaired == missing)) || fail "only $repaired of $missing missing transactions were fetched from RPC"
say "PASS C: patch off → verification caught $missing dropped at the handoff (all fetched from RPC); patch on → complete"
