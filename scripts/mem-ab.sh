#!/usr/bin/env bash
# A/B memory experiment over config knobs, using an isolated portable install.
# Never touches the real %APPDATA%\NookSill\NookSill.ini.
#   ./scripts/mem-ab.sh                     # run the default matrix
#   VARIANTS="base no_preload" ./scripts/mem-ab.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# shellcheck source=env.sh
source "$ROOT/scripts/env.sh"

DURATION="${DURATION:-40}"
INTERVAL="${INTERVAL:-4}"
WARMUP="${WARMUP:-20}"
VARIANTS="${VARIANTS:-base no_preload no_frost no_smoke no_preload_no_frost lean}"

SANDBOX="$ROOT/scratch/mem-ab-sandbox"
mkdir -p "$SANDBOX"

# Build once, then copy into the sandbox with the portable marker.
./scripts/build.sh --release -p nooksill-gpu --bin nooksill-dock >/dev/null
SRC_EXE="$(./scripts/build.sh --release --exe)"
cp -f "$SRC_EXE" "$SANDBOX/nooksill-dock.exe"
: > "$SANDBOX/NookSill.portable"

# Base ini mirrors a clean default install. Each variant overrides a few keys.
write_ini() {
    local name="$1"
    local ini="$SANDBOX/NookSill.ini"
    printf '[meta]\nversion=6\n\n' > "$ini"
    printf '[general]\nLanguage=en\n\n' >> "$ini"
    printf '[effect]\nBgMode=2\nBlur=6\n\n' >> "$ini"
    printf '[effect.smoke]\nSmoke=1\n\n' >> "$ini"
    printf '[widgets]\nPreloadPanels=1\nGlowStrength=50\n\n' >> "$ini"
    case "$name" in
        no_preload)        sed -i 's/PreloadPanels=1/PreloadPanels=0/' "$ini" ;;
        no_frost)          sed -i 's/BgMode=2/BgMode=0/' "$ini" ;;
        no_smoke)          sed -i 's/Smoke=1/Smoke=0/' "$ini" ;;
        no_preload_no_frost) sed -i -e 's/PreloadPanels=1/PreloadPanels=0/' -e 's/BgMode=2/BgMode=0/' "$ini" ;;
        lean)              sed -i -e 's/PreloadPanels=1/PreloadPanels=0/' -e 's/BgMode=2/BgMode=0/' -e 's/Smoke=1/Smoke=0/' -e 's/GlowStrength=50/GlowStrength=0/' "$ini" ;;
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

printf '%-22s %10s %10s %8s\n' variant ws_mb private_mb samples
printf '%-22s %10s %10s %8s\n' ------- ------ ---------- -------

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
        printf '%-22s %10s %10s %8d\n' "$v" DEAD - 0
        continue
    fi
    # Median of post-warmup WS samples.
    ws=$(printf '%s\n' "${values[@]}" | sort -n | awk '{a[NR]=$1} END{print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}')
    printf '%-22s %10s %10s %8d\n' "$v" "$ws" - "${#values[@]}"
done

trap kill_dock EXIT
