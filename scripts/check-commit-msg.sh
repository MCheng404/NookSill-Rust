#!/usr/bin/env bash
# Commit messages are ASCII-only English. Git passes the message file to the
# commit-msg hook; CI passes the subject and body of HEAD the same way.

set -uo pipefail

NON_ASCII='[^\x00-\x7F]'

# Strip Git's own comments and everything after the scissors line.
message_body() {
    awk '
        /^# ------------------------ >8 ------------------------/ { exit }
        /^#/ { next }
        { print }
    ' "$1"
}

violations() {
    message_body "$1" | grep -nP "$NON_ASCII" 2>/dev/null || true
}

self_test() {
    local tmp ok
    tmp="$(mktemp -d)"
    cat > "$tmp/ok" <<'EOF'
feat: add a configurable toggle hotkey

Body stays in English and may use ASCII punctuation: [Graphics] Adapter.
EOF
    cat > "$tmp/bad" <<'EOF'
feat: add a configurable hotkey
中文正文
EOF
    cat > "$tmp/comments" <<'EOF'
fix: keep the dock on the left edge

# 中文注释行应当被忽略
# Please enter the commit message for your changes.
EOF
    cat > "$tmp/scissors" <<'EOF'
docs: sync the configuration table
# ------------------------ >8 ------------------------
# 中文 diff 内容应当被忽略
EOF

    for f in ok comments scissors; do
        if [ -n "$(violations "$tmp/$f")" ]; then
            echo "commit message self-test failed: $f should pass" >&2
            rm -rf "$tmp"
            return 1
        fi
    done
    ok="$(violations "$tmp/bad")"
    if [ -z "$ok" ]; then
        echo "commit message self-test failed: bad sample was accepted" >&2
        rm -rf "$tmp"
        return 1
    fi
    rm -rf "$tmp"
    echo "commit message gate self-test ok (ASCII, comments, scissors)."
}

if [ "${1:-}" = "--self-test" ]; then
    self_test
    exit $?
fi

if [ $# -lt 1 ] || [ ! -f "$1" ]; then
    echo "usage: check-commit-msg.sh <message-file> | --self-test" >&2
    exit 1
fi

hits="$(violations "$1")"
if [ -n "$hits" ]; then
    echo "commit message must be ASCII-only English:" >&2
    printf '%s\n' "$hits" >&2
    exit 1
fi
exit 0
