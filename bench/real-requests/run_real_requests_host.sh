#!/usr/bin/env bash
# Host-only: bounded real INNER capture and the existing CPU20/thread PROBE.
# It intentionally does not invoke SSH and refuses to start after the card cutoff.
set -euo pipefail
CUTOFF=1788756895
LEFT=$((CUTOFF-$(date +%s)))
((LEFT>0)) || exit 124
if [[ ${REAL_REQUESTS_LOCKED:-0} != 1 ]]; then
 exec flock -n "$HOME/.cache/uni-rnaseq-resource.lock" timeout --signal=TERM --kill-after=20s "${LEFT}s" env REAL_REQUESTS_LOCKED=1 bash "$0"
fi
exec python3 -B "$(dirname "$0")/run_host.py"
