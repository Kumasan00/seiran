#!/usr/bin/env bash
# frontend（字句・構文解析・評価）を cargo-fuzz で探索する。ローカルと CI（.github/workflows/fuzz.yml）が共用する。
#
# 上限はすべて探索の予算で、製品の入力上限ではない:
# - 第 1 引数: 全体の秒数（-max_total_time。既定 60）
# - -timeout=10: 1 入力 10 秒を超えたら timeout-* として保存する
# - -rss_limit_mb=2048: 2 GB を超えたら oom-* として保存する
# - -max_len=8192: 1 入力 8192 バイト（tests/text/perf の大きい fixture も切り詰めて読む）
# 第 2 引数以降は libFuzzer へそのまま渡す（後ろに付くので上の既定を上書きできる）。
#
# 新入力は先頭の fuzz/corpus/frontend だけに書かれ、fuzz/seeds/frontend と tests/text は読むだけ。
# fuzz バイナリの cwd は cargo-fuzz が決めるので、パスは絶対パスで渡す。
#
# 使い方: tools/fuzz-frontend.sh [秒数] [libFuzzer の追加引数...]（手順は fuzz/README.md）
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
seconds="${1:-60}"
if [ "$#" -gt 0 ]; then
  shift
fi
mkdir -p "$root/fuzz/corpus/frontend"
exec cargo +nightly fuzz run frontend "$root/fuzz/corpus/frontend" "$root/fuzz/seeds/frontend" "$root/tests/text" -- \
  -dict="$root/fuzz/frontend.dict" -max_total_time="$seconds" \
  -timeout=10 -rss_limit_mb=2048 -max_len=8192 "$@"
