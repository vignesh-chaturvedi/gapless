#!/usr/bin/env bash
# Run scenarios A–D in order, ROUNDS times (default 2). Stops at the first failure; an
# inconclusive run (Solami behaved perfectly, so there was nothing to catch) is retried.
#   scripts/scenarios.sh            # a b c d, twice
#   scripts/scenarios.sh a b        # just these, twice
set -uo pipefail
dir=$(dirname "$0")
ROUNDS=${ROUNDS:-2}
list=("$@")
((${#list[@]})) || list=(a b c d)
for round in $(seq 1 "$ROUNDS"); do
  for s in "${list[@]}"; do
    name=$(echo "$s" | tr a-z A-Z)
    echo "── round $round, scenario $name ──"
    # Inconclusive (exit 2) means Solami didn't misbehave that time; try up to three times.
    for attempt in 1 2 3; do
      "$dir/scenario-$s.sh"
      code=$?
      ((code == 2)) || break
      echo "── scenario $name was inconclusive (attempt $attempt) ──"
    done
    ((code == 0)) || exit "$code"
  done
done
echo "── all passed, $ROUNDS round(s) ──"
