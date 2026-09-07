#!/usr/bin/env bash
# Reuse real-requests-host1; run only conversion and the probe under renewed grant.
set -euo pipefail
CUTOFF=1788756895
LEFT=$((CUTOFF-$(date +%s)))
((LEFT>0)) || exit 124
if [[ ${REAL_REQUESTS_LOCKED:-0} != 1 ]]; then
 exec flock -n "$HOME/.cache/uni-rnaseq-resource.lock" timeout --signal=TERM --kill-after=20s "${LEFT}s" env REAL_REQUESTS_LOCKED=1 bash "$0"
fi
exec python3 -B "$(dirname "$0")/resume_host.py"
