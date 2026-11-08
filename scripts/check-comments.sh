#!/usr/bin/env bash
# Comment language gate: every `//` comment line in crates/ must be ASCII.
# Rule lives in CONTRIBUTING.md, section 6.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

hits=$(grep -rnP '^\s*//+.*[^\x00-\x7F]' crates --include='*.rs' || true)

if [ -n "$hits" ]; then
    echo "非 ASCII 注释：$(printf '%s\n' "$hits" | wc -l) 处" >&2
    printf '%s\n' "$hits" | head -40 >&2
    exit 1
fi

echo "注释检查通过。"
