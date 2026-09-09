#!/usr/bin/env bash
set -euo pipefail
[[ $# -ge 5 && $# -le 6 ]] || { echo "usage: $0 STOCK INTEGRATED BASE_ARGV_JSON OUTPUT TIMEOUT_S [REPEATS]" >&2; exit 64; }
here=$(cd "$(dirname "$0")" && pwd)
exec python3 -B "$here/measurement_runner.py" --stock "$1" --integrated "$2" --base "$3" \
  --output "$4" --timeout-s "$5" --repeats "${6:-1}" --profile \
  --profile-read-limit "${STAR_INTEGRATE_PROFILE_READ_LIMIT:?explicit reduced-slice read limit required}"
