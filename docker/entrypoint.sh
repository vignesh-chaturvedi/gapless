#!/bin/sh
# Live when SOLAMI_API_KEY is set, otherwise the recorded mainnet fixture. GAPLESS_PUBLIC=1 runs
# it as a hosted demo (outages rationed); PORT overrides 8790 for hosts that assign one. Extra
# arguments go to gapless-server (for example `--horizon 200` offline).
set -e
port="${PORT:-8790}"
common="--listen 0.0.0.0:$port --db /data/gapless.db --console /app/console"
case "$GAPLESS_PUBLIC" in
  1 | true | yes) common="$common --public"; echo "gapless: public demo: one outage every 5 minutes, at most 30 s offline" ;;
esac
if [ -n "$SOLAMI_API_KEY" ]; then
  echo "gapless: SOLAMI_API_KEY is set; streaming live from Solami. Console: http://localhost:$port"
  exec gapless-server $common "$@"
fi
echo "gapless: no SOLAMI_API_KEY; playing the recorded mainnet fixture (offline). Console: http://localhost:$port"
exec gapless-server --offline /app/fixtures/pumpfun-150s.bin.zst $common "$@"
