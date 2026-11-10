#!/usr/bin/env bash
# File size gate: one .rs file has one responsibility.
# Production code is capped at 600 lines; a trailing #[cfg(test)] module is not
# counted as production, but a dedicated test file is capped at the same limit.
# Rule lives in CONTRIBUTING.md, section 7.
#
# Usage:
#   ./scripts/check-size.sh [root]   # default: crates/
#   ./scripts/check-size.sh --self-test
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

LIMIT=600

is_test_file() {
    case "$1" in
        */tests/*|*/tests.rs) return 0 ;;
        *) return 1 ;;
    esac
}

# Echo "file:effective_lines" for every violation.
violations() {
    local root="$1" f total cfg prod
    while IFS= read -r f; do
        total="$(wc -l < "$f")"
        if is_test_file "$f"; then
            prod="$total"
        else
            cfg="$(grep -nE -m1 '^[[:space:]]*#\[cfg\(test\)\]' "$f" | cut -d: -f1 || true)"
            if [ -n "$cfg" ]; then
                prod=$((cfg - 1))
            else
                prod="$total"
            fi
        fi
        if [ "$prod" -gt "$LIMIT" ]; then
            printf '%s: %s lines (limit %s)\n' "$f" "$prod" "$LIMIT"
        fi
    done < <(find "$root" -name '*.rs' -type f | sort)
}

self_test() {
    local tmp big small hits
    tmp="$(mktemp -d)"
    awk 'BEGIN { for (i = 0; i < 601; i++) print "// filler" }' > "$tmp/big.rs"
    awk 'BEGIN { for (i = 0; i < 600; i++) print "// filler" }' > "$tmp/small.rs"
    hits="$(violations "$tmp")"
    rm -rf "$tmp"
    big="$(printf '%s\n' "$hits" | grep -c 'big.rs' || true)"
    small="$(printf '%s\n' "$hits" | grep -c 'small.rs' || true)"
    if [ "$big" -ne 1 ] || [ "$small" -ne 0 ]; then
        echo "size gate self-test failed: big=$big small=$small" >&2
        printf '%s\n' "$hits" >&2
        return 1
    fi
    echo "size gate self-test ok (601 fails, 600 passes)."
}

if [ "${1:-}" = "--self-test" ]; then
    self_test
    exit 0
fi

target="${1:-crates}"
if [ ! -e "$target" ]; then
    echo "path not found: $target" >&2
    exit 1
fi

hits="$(violations "$target")"
if [ -n "$hits" ]; then
    echo "oversized .rs files:" >&2
    printf '%s\n' "$hits" >&2
    exit 1
fi
echo "file size check passed."
