#!/usr/bin/env bash
# Full-process memory guard for the dock, distinct from the device-level probe.
# Modes: quick (5 min regression check), soak (4 h leak guard), measure (feat delta).
#   MODE=quick DURATION=300 ./scripts/mem-guard.sh
#   MODE=soak  DURATION=14400 ./scripts/mem-guard.sh
# Sample口径 matches mem.rs: WorkingSet64 / PrivateMemorySize64 / PeakWorkingSet64.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=env.sh
source "$ROOT/scripts/env.sh"

MODE="${MODE:-quick}"
DURATION="${DURATION:-300}"
INTERVAL="${INTERVAL:-10}"
WARMUP="${WARMUP:-60}"
RELEASE="${RELEASE:-0}"
OUT="${OUT:-$ROOT/scratch/mem-${MODE}-$(date +%Y%m%d-%H%M%S).csv}"
mkdir -p "$(dirname "$OUT")"

# Build and resolve the exe (build.sh owns the target triple and profile).
PF=()
[ "$RELEASE" = 1 ] && PF=(--release)
./scripts/build.sh "${PF[@]+"${PF[@]}"}" -p nooksill-gpu --bin nooksill-dock >/dev/null
EXE="$(./scripts/build.sh "${PF[@]+"${PF[@]}"}" --exe)"

# Single-instance mutex (ADR-015): a stale instance makes the new process exit at once.
kill_dock() { MSYS_NO_PATHCONV=1 taskkill /IM nooksill-dock.exe /F >/dev/null 2>&1 || true; }
kill_dock
"$EXE" &
trap kill_dock EXIT

# Emit one CSV row: ws_mb,private_mb,peak_ws_mb. PowerShell owns the counters.
sample() {
    powershell.exe -NoProfile -NonInteractive -Command '
        $p = Get-Process -Name nooksill-dock -ErrorAction SilentlyContinue | Select-Object -First 1;
        if ($null -eq $p) { "DEAD" } else {
            "{0},{1},{2}" -f [math]::Round($p.WorkingSet64/1MB,2),
                             [math]::Round($p.PrivateMemorySize64/1MB,2),
                             [math]::Round($p.PeakWorkingSet64/1MB,2)
        }' | tr -d '\r'
}

printf 't_sec,ws_mb,private_mb,peak_ws_mb\n' > "$OUT"
start=$(date +%s)
while :; do
    el=$(( $(date +%s) - start ))
    [ "$el" -ge "$DURATION" ] && break
    line="$(sample)"
    if [ "$line" = "DEAD" ]; then
        echo "dock exited early (single-instance or crash?)" >&2
        break
    fi
    printf '%s,%s\n' "$el" "$line" >> "$OUT"
    sleep "$INTERVAL"
done

# Thresholds: soak guards slow leaks; quick only catches gross regressions.
if [ "$MODE" = soak ]; then
    THR_SLOPE=5; THR_R2=0.50; THR_DELTA=20; THR_PEAK=30
else
    THR_SLOPE=60; THR_R2=0.60; THR_DELTA=25; THR_PEAK=40
fi

# Linear regression on the post-warmup window plus first/last quintile means.
# R² is required so random walk is not reported as monotonic growth.
awk -F, -v drop="$WARMUP" -v mode="$MODE" -v csv="$OUT" \
    -v ts="$THR_SLOPE" -v tr="$THR_R2" -v td="$THR_DELTA" -v tp="$THR_PEAK" '
    NR==1 { next }
    $1 < drop { next }
    {
        n++; x[n]=$1/60.0; y[n]=$2; pk[n]=$4
        sx+=x[n]; sy+=y[n]; sxx+=x[n]*x[n]; sxy+=x[n]*y[n]
    }
    END {
        if (n<5) { printf "%s: too few samples (%d), no verdict\n", mode, n; exit 2 }
        den=n*sxx-sx*sx
        if (den<=0) { print "time span too short"; exit 2 }
        slope=(n*sxy-sx*sy)/den
        inter=(sy-slope*sx)/n
        my=sy/n; sst=0; sse=0
        for (i=1;i<=n;i++) {
            fit=inter+slope*x[i]
            sst+=(y[i]-my)*(y[i]-my); sse+=(y[i]-fit)*(y[i]-fit)
        }
        r2 = sst>0 ? 1-sse/sst : 0
        k=int(n/5); if (k<2) k=2; f=0; l=0
        for (i=1;i<=k;i++) f+=y[i]
        for (i=n-k+1;i<=n;i++) l+=y[i]
        delta=l/k-f/k
        peak_growth=pk[n]-pk[1]

        printf "%s: n=%d slope=%.2f MB/h R2=%.3f first-last=%.1f MB peak_growth=%.1f MB\n", \
               mode,n,slope,r2,delta,peak_growth
        printf "CSV: %s\n", csv

        if (slope>ts && r2>tr) { print "FAIL: working set grows monotonically"; exit 1 }
        if (delta>td && r2>tr) { print "FAIL: first/last window gap too large"; exit 1 }
        if (peak_growth>tp)    { print "FAIL: peak working set keeps growing"; exit 1 }
        print "PASS: steady state"; exit 0
    }
' "$OUT"
