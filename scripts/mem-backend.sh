#!/usr/bin/env bash
# A/B over wgpu backends with the user's real config, using NOOKSILL_BACKEND.
# Isolated portable install; never touches %APPDATA%\NookSill\NookSill.ini.
#   ./scripts/mem-backend.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source "$ROOT/scripts/env.sh"

DURATION="${DURATION:-50}"
INTERVAL="${INTERVAL:-5}"
WARMUP="${WARMUP:-30}"
# dx12/vulkan/gl each pinned to NVIDIA (the user's actual adapter choice).
VARIANTS="${VARIANTS:-dx12_nvidia vulkan_nvidia gl_nvidia dx12_amd vulkan_amd}"

SANDBOX="$ROOT/scratch/mem-backend-sandbox"
mkdir -p "$SANDBOX"
./scripts/build.sh --release -p nooksill-gpu --bin nooksill-dock >/dev/null
cp -f "$(./scripts/build.sh --release --exe)" "$SANDBOX/nooksill-dock.exe"
: > "$SANDBOX/NookSill.portable"

write_ini() {
    local adapter="$1"
    local ini="$SANDBOX/NookSill.ini"
    printf '[meta]\nversion=6\n\n' > "$ini"
    printf '[general]\nLanguage=en\n\n' >> "$ini"
    printf '[appearance]\nicon_size=56\nzoom_size=40\nblur=14\nfill=2\nreflect=1\n\n' >> "$ini"
    printf '[effect]\nBgMode=2\nBlur=14\n\n' >> "$ini"
    printf '[effect.smoke]\nSmoke=0\nSmokeClouds=0\n\n' >> "$ini"
    printf '[widgets]\nPreloadPanels=1\nGlowStrength=50\n\n' >> "$ini"
    printf '[performance]\nMsaa=4\n\n' >> "$ini"
    printf '[Graphics]\nAdapter=%s\n\n' "$adapter" >> "$ini"
}

kill_dock() { MSYS_NO_PATHCONV=1 taskkill /IM nooksill-dock.exe /F >/dev/null 2>&1 || true; }

sample_once() {
    powershell.exe -NoProfile -NonInteractive -Command '
        $p = Get-Process -Name nooksill-dock -ErrorAction SilentlyContinue | Select-Object -First 1;
        if ($null -eq $p) { "DEAD" } else {
            "{0},{1}" -f [math]::Round($p.WorkingSet64/1MB,2),
                        [math]::Round($p.PrivateMemorySize64/1MB,2)
        }' | tr -d '\r'
}

printf '%-20s %10s %10s %8s\n' variant ws_mb private_mb samples
printf '%-20s %10s %10s %8s\n' ------- ------ ---------- -------

for v in $VARIANTS; do
    backend="${v%%_*}"
    adapter="${v#*_}"
    kill_dock
    write_ini "$adapter"
    (cd "$SANDBOX" && NOOKSILL_BACKEND="$backend" ./nooksill-dock.exe >/dev/null 2>&1) &
    sleep 3
    values=()
    start=$(date +%s)
    while :; do
        el=$(( $(date +%s) - start ))
        [ "$el" -ge "$DURATION" ] && break
        line="$(sample_once)"
        [ "$line" = "DEAD" ] && break
        if [ "$el" -ge "$WARMUP" ]; then values+=("${line%%,*}"); fi
        sleep "$INTERVAL"
    done
    kill_dock
    sleep 2
    if [ ${#values[@]} -eq 0 ]; then
        printf '%-20s %10s %10s %8d\n' "$v" DEAD - 0
        continue
    fi
    ws=$(printf '%s\n' "${values[@]}" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}')
    printf '%-20s %10s %10s %8d\n' "$v" "$ws" - "${#values[@]}"
done

trap kill_dock EXIT
