#!/bin/sh
# Live when SOLAMI_API_KEY is set, otherwise the recorded mainnet fixture. Extra arguments go to
# gapless-server (for example `--horizon 200` offline).
set -e
common="--listen 0.0.0.0:8790 --db /data/gapless.db --console /app/console"
if [ -n "$SOLAMI_API_KEY" ]; then
  echo "gapless: SOLAMI_API_KEY is set; streaming live from Solami. Console: http://localhost:8790"
  exec gapless-server $common "$@"
fi
echo "gapless: no SOLAMI_API_KEY; playing the recorded mainnet fixture (offline). Console: http://localhost:8790"
exec gapless-server --offline /app/fixtures/pumpfun-150s.bin.zst $common "$@"
