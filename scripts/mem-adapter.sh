#!/usr/bin/env bash
# A/B over adapter and MSAA using the user's real config values.
# Isolated portable install: never touches %APPDATA%\NookSill\NookSill.ini.
#   ./scripts/mem-adapter.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source "$ROOT/scripts/env.sh"

DURATION="${DURATION:-40}"
INTERVAL="${INTERVAL:-4}"
WARMUP="${WARMUP:-20}"
VARIANTS="${VARIANTS:-default_gpu_msaa4 nvidia_msaa4 nvidia_msaa2 nvidia_msaa1 amd_msaa4}"

SANDBOX="$ROOT/scratch/mem-adapter-sandbox"
mkdir -p "$SANDBOX"
./scripts/build.sh --release -p nooksill-gpu --bin nooksill-dock >/dev/null
cp -f "$(./scripts/build.sh --release --exe)" "$SANDBOX/nooksill-dock.exe"
: > "$SANDBOX/NookSill.portable"

write_ini() {
    local name="$1"
    local ini="$SANDBOX/NookSill.ini"
    local msaa=4
    case "$name" in
        nvidia_msaa2) msaa=2 ;;
        nvidia_msaa1) msaa=1 ;;
    esac
    printf '[meta]\nversion=6\n\n' > "$ini"
    printf '[general]\nLanguage=en\n\n' >> "$ini"
    printf '[appearance]\nicon_size=56\nzoom_size=40\nblur=14\nfill=2\nreflect=1\n\n' >> "$ini"
    printf '[effect]\nBgMode=2\nBlur=14\n\n' >> "$ini"
    printf '[effect.smoke]\nSmoke=0\nSmokeClouds=0\n\n' >> "$ini"
    printf '[widgets]\nPreloadPanels=1\nGlowStrength=50\n\n' >> "$ini"
    printf '[performance]\nMsaa=%s\n\n' "$msaa" >> "$ini"
    case "$name" in
        default_gpu*) ;;                     # no [Graphics] -> default LowPower (iGPU)
        amd_msaa4) printf '[Graphics]\nAdapter=AMD\n\n' >> "$ini" ;;
        nvidia_*)  printf '[Graphics]\nAdapter=NVIDIA\n\n' >> "$ini" ;;
    esac
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
    kill_dock
    write_ini "$v"
    (cd "$SANDBOX" && ./nooksill-dock.exe >/dev/null 2>&1) &
    sleep 2
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
    sleep 1
    if [ ${#values[@]} -eq 0 ]; then
        printf '%-20s %10s %10s %8d\n' "$v" DEAD - 0
        continue
    fi
    ws=$(printf '%s\n' "${values[@]}" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}')
    printf '%-20s %10s %10s %8d\n' "$v" "$ws" - "${#values[@]}"
done

trap kill_dock EXIT
