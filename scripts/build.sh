#!/usr/bin/env bash
# Build entry point. scripts/env.sh injects MSVC and SDK paths; this script
# only picks which cargo command to run.
#
# Usage:
#   ./scripts/build.sh                # debug build
#   ./scripts/build.sh --release      # release build (size + LTO)
#   ./scripts/build.sh --run          # build and run
#   ./scripts/build.sh --test         # all unit and integration tests
#   ./scripts/build.sh --test config  # tests whose name contains config
#   ./scripts/build.sh --check        # cargo check (fastest feedback)
#   ./scripts/build.sh --clippy       # clippy -D warnings
#   ./scripts/build.sh --fmt          # format check
#   ./scripts/build.sh --gate         # comments, size, fmt, clippy, tests
#   ./scripts/build.sh --exe          # print the binary path
#   ./scripts/build.sh --version      # print the pinned build version
#   ./scripts/package.sh [--release]  # build and package into dist/
#
# Other arguments are passed through to cargo (e.g. -p <crate>, --nocapture).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# shellcheck source=env.sh
source "$ROOT/scripts/env.sh"

MODE=debug
ACTION=build
EXTRA=()
while [ $# -gt 0 ]; do
    case "$1" in
        --release) MODE=release; shift ;;
        --run)     ACTION=run; shift ;;
        --test)    ACTION=test; shift ;;
        --check)   ACTION=check; shift ;;
        --clippy)  ACTION=clippy; shift ;;
        --fmt)     ACTION=fmt; shift ;;
        --gate)    ACTION=gate; shift ;;
        --exe)     ACTION=exe; shift ;;
        --version) ACTION=version; shift ;;
        *) EXTRA+=("$1"); shift ;;
    esac
done

profile_flag() {
    [ "$MODE" = release ] && echo --release
}

profile_dir() {
    if [ "$MODE" = release ]; then echo release; else echo debug; fi
}

case "$ACTION" in
    build)
        cargo build --target "$TARGET" $(profile_flag) ${EXTRA[@]+"${EXTRA[@]}"}
        ;;
    run)
        cargo run --target "$TARGET" $(profile_flag) ${EXTRA[@]+"${EXTRA[@]}"}
        ;;
    test)
        # --lib --tests：跳过 doctest。本项目的文档示例不承担 API 契约，
        # 每个 doctest 都是独立的编译+运行目标，纯属浪费链接时间。
        cargo test --target "$TARGET" --workspace --lib --tests ${EXTRA[@]+"${EXTRA[@]}"}
        ;;
    check)
        cargo check --target "$TARGET" --workspace --all-targets ${EXTRA[@]+"${EXTRA[@]}"}
        ;;
    clippy)
        cargo clippy --target "$TARGET" --workspace --all-targets -- -D warnings ${EXTRA[@]+"${EXTRA[@]}"}
        ;;
    fmt)
        cargo fmt --all -- --check
        ;;
    gate)
        # All steps run before reporting, so one pass shows every problem.
        status=0
        echo "== 1/5 comments =="
        ./scripts/check-comments.sh --self-test && ./scripts/check-comments.sh || status=1
        echo "== 2/5 size =="
        ./scripts/check-size.sh --self-test && ./scripts/check-size.sh || status=1
        echo "== 3/5 fmt =="
        cargo fmt --all -- --check || status=1
        echo "== 4/5 clippy =="
        cargo clippy --target "$TARGET" --workspace --all-targets -- -D warnings ${EXTRA[@]+"${EXTRA[@]}"} || status=1
        echo "== 5/5 test =="
        cargo test --target "$TARGET" --workspace --lib --tests ${EXTRA[@]+"${EXTRA[@]}"} || status=1
        echo
        if [ "$status" -eq 0 ]; then
            echo "门禁通过。"
        else
            echo "门禁未通过，见上面的输出。" >&2
        fi
        exit "$status"
        ;;
    exe)
        # ADR-035 之后唯一的 Dock 可执行文件；早期这里写的是已删除的 NookSill.exe。
        echo "$ROOT/target/$TARGET/$(profile_dir)/nooksill-dock.exe"
        ;;
    version)
        # 由构建脚本写出（`crates/nooksill-core/build.rs`），与 exe 资源同源。
        f="$ROOT/target/$TARGET/$(profile_dir)/NOOKSILL_VERSION"
        if [ -f "$f" ]; then
            cat "$f"
        else
            echo "尚未构建：先跑一次 ./scripts/build.sh" >&2
            exit 1
        fi
        ;;
esac
