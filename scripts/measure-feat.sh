#!/usr/bin/env bash
# Measure the resident-memory delta of a feature against a saved baseline.
# Both runs must use the same box, profile, scenario and warmup window.
#   ./scripts/measure-feat.sh scratch/mem-baseline.csv
# Baseline: MODE=measure DURATION=180 WARMUP=60 ./scripts/mem-guard.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BASE="${1:?usage: measure-feat.sh <baseline.csv>}"
[ -f "$BASE" ] || { echo "baseline not found: $BASE" >&2; exit 2; }

DURATION="${DURATION:-180}"
INTERVAL="${INTERVAL:-5}"
WARMUP="${WARMUP:-60}"
CUR="scratch/mem-feat-$(date +%s).csv"

MODE=measure DURATION="$DURATION" INTERVAL="$INTERVAL" WARMUP="$WARMUP" \
    OUT="$ROOT/$CUR" "$ROOT/scripts/mem-guard.sh" >/dev/null

# Median over the post-warmup window: robust against driver jitter (BUILD.md:230).
median_ws() {
    awk -F, -v d="$WARMUP" 'NR>1 && $1>=d {print $2}' "$1" \
        | sort -n | awk '{a[NR]=$1} END {print (NR%2) ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2}'
}

b_ws=$(median_ws "$BASE")
c_ws=$(median_ws "$CUR")
awk -v b="$b_ws" -v c="$c_ws" -v d="$DURATION" -v w="$WARMUP" 'BEGIN {
    printf "Memory delta: WS %+.1f MB (baseline %.1f, current %.1f; idle %ss; warmup %ss; steady median)\n", \
           c-b, b, c, d, w
}'
