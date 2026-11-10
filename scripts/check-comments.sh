#!/usr/bin/env bash
# Comment language gate: comment text must be ASCII, strings may be any language.
# Rule lives in CONTRIBUTING.md, section 6.
#
# Usage:
#   ./scripts/check-comments.sh [root]   # default: crates/
#   ./scripts/check-comments.sh --self-test
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# String literals are consumed as one token so "http://..." is not a comment.
LINE_RE='^(?:[^"]|"(?:\\.|[^"\\])*")*//.*[^\x00-\x7F]'
NON_ASCII='[^\x00-\x7F]'

line_comments() {
    grep -rnP "$LINE_RE" "$1" --include='*.rs' 2>/dev/null || true
}

# awk only extracts text inside /* ... */; grep decides whether it is ASCII.
block_comments() {
    find "$1" -name '*.rs' -type f -print0 \
        | xargs -0 -r awk '
            FNR == 1 { inblock = 0 }
            function emit(s) { print FILENAME ":" FNR ":" s }
            {
                line = $0
                if (inblock) {
                    e = index(line, "*/")
                    if (e) { emit(substr(line, 1, e - 1)); inblock = 0; line = substr(line, e + 2) }
                    else { emit(line); next }
                }
                s = index(line, "/*")
                while (s) {
                    tail = substr(line, s + 2)
                    e = index(tail, "*/")
                    if (e) { emit(substr(tail, 1, e - 1)); line = substr(tail, e + 2); s = index(line, "/*") }
                    else { emit(tail); inblock = 1; break }
                }
            }' \
        | grep -P "$NON_ASCII" || true
}

violations() {
    { line_comments "$1"; block_comments "$1"; } | sort -u
}

self_test() {
    local tmp hits n
    tmp="$(mktemp -d)"
    cat > "$tmp/sample.rs" <<'EOF'
let url = "http://example.com";  // ascii comment
let s = "中文字符串";
let x = 1; // 中文行尾注释
/// 中文文档注释
//! 中文模块注释
/* 中文块注释 */
/* ascii start
中文续行 */
EOF
    hits="$(violations "$tmp")"
    n="$(printf '%s\n' "$hits" | grep -c . || true)"
    rm -rf "$tmp"
    if [ "$n" -ne 5 ]; then
        echo "comment gate self-test failed: expected 5 hits, got $n" >&2
        printf '%s\n' "$hits" >&2
        return 1
    fi
    echo "comment gate self-test ok (line-tail, doc, module, single and multi-line block)."
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
    echo "non-ASCII comments: $(printf '%s\n' "$hits" | grep -c .) lines" >&2
    printf '%s\n' "$hits" | head -40 >&2
    exit 1
fi
echo "comment check passed."
