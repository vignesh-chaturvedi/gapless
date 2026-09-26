# Shared helpers for the chaos scenarios. Needs curl and jq, and a running gapless-server.
# shellcheck shell=bash

GAPLESS_URL=${GAPLESS_URL:-http://127.0.0.1:8790}

api() { curl -fsS "$GAPLESS_URL$1"; }
post() { curl -fsS -X POST "$GAPLESS_URL$1" -H 'content-type: application/json' -d "$2"; }
# Progress goes to stderr, so functions can print results on stdout for $(...).
say() { printf '%s  %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
fail() { say "FAIL: $*"; exit 1; }

need() {
  for tool in curl jq; do
    command -v "$tool" >/dev/null || { echo "needs $tool"; exit 1; }
  done
  api /api/health >/dev/null 2>&1 || { echo "gapless-server isn't answering at $GAPLESS_URL"; exit 1; }
}

mode() { api /api/health | jq -r .mode; }

# Wait (up to $1 seconds) until the stream is live, and in live mode until Solami has listed it.
wait_live() {
  local deadline=$((SECONDS + ${1:-120}))
  while ((SECONDS < deadline)); do
    local s; s=$(api /api/state)
    if [[ $(jq -r .state.kind <<<"$s") == live ]] &&
      { [[ $(jq -r .mode <<<"$s") == offline ]] || [[ $(jq -r .controls.canKill <<<"$s") == true ]]; } &&
      [[ $(jq -r '.openIncident == null' <<<"$s") == true ]]; then
      return 0
    fi
    sleep 2
  done
  fail "the stream wasn't live and settled within ${1:-120}s"
}

latest_incident() { api '/api/incidents?limit=1' | jq -r '.[0].id // 0'; }

# Wait for an incident newer than $1; print its id.
wait_new_incident() {
  local after=$1 deadline=$((SECONDS + ${2:-120}))
  while ((SECONDS < deadline)); do
    local id; id=$(latest_incident)
    if ((id > after)); then echo "$id"; return 0; fi
    sleep 2
  done
  fail "no incident opened within ${2:-120}s"
}

# Wait for incident $1 to finish verification; print its JSON.
wait_verified() {
  local id=$1 deadline=$((SECONDS + ${2:-600})) last=""
  while ((SECONDS < deadline)); do
    local inc; inc=$(api "/api/incidents/$id")
    local status; status=$(jq -r '.verification.status // "none"' <<<"$inc")
    [[ $status != "$last" ]] && say "incident #$id: $(jq -r .status <<<"$inc"), verification $status"
    last=$status
    case $status in done | failed | skipped) echo "$inc"; return 0 ;; esac
    sleep 3
  done
  fail "incident #$id wasn't verified within ${2:-600}s"
}

# One-paragraph summary of an incident.
describe() {
  jq -r '
    def n: tostring | if length > 3 then .[:-3] + "," + .[-3:] else . end;
    "incident #\(.id): \(.reason.text)" +
    (if .chaos then " (\(.chaos))" else "" end),
    "  gap: \(if .gap then "\(.gap.last - .gap.first + 1) slots (\(.gap.first)–\(.gap.last))" else "none" end)" +
    (if .unrecoverable then ", LOST beyond the horizon: \(.unrecoverable.last - .unrecoverable.first + 1) slots (\(.unrecoverable.first)–\(.unrecoverable.last))" else "" end),
    "  replayed \(.replayed) in \(.steps | length) step(s), \(.duplicates) duplicates dropped, recovered in \((.durationMs // 0) / 1000 | floor)s",
    "  handoff patch: \(if .patch then "re-read \(.patch.slots.first)–\(.patch.slots.last), recovered \(.patch.recovered)" else "off" end)",
    (if .verification.report then
      .verification.report as $r |
      "  verified \($r.range.first)–\($r.range.last): \($r.verdict.kind), \($r.matched) / \($r.expected) expected" +
      (if ($r.missing | length) > 0 then ", \($r.missing | length) missing (\([$r.missing[].cause] | group_by(.) | map("\(length) \(.[0] // "unexplained")") | join(", "))), \($r.repaired | length) fetched from RPC" else "" end)
    else "  verification: \(.verification.status // "none") \(.verification.error // "")" end)
  '
}
