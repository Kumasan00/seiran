#!/usr/bin/env bash
# 性能 baseline 計測スクリプト（issue #253 / #252 step1）。
#
# `build_pdf` を build driver / compiler core に分離するリファクタ（#252）の各 step 前後で
# 目視比較できるよう、代表的な小・中・大規模文書について compile 時間（段階別 + 全体）・
# peak memory・出力 PDF サイズを記録する。CI で assert する性能テストではない（機種依存で
# フレークするため）。値は docs/perf-baseline.md へ手動で転記する運用とする。
#
# 前提: tools/fetch-test-assets.sh 済み（vendor/ にフォント等が取得済み）。
# 使い方: tools/perf-baseline.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BASE_CONFIG="crates/seiran/tests/config/config.toml"
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

echo "== release ビルド =="
cargo build --release

measure() {
  local tier="$1"
  local source="$2"
  local config="$WORKDIR/${tier}.toml"
  local log="$WORKDIR/${tier}.log"

  # `sources` だけをこの tier の入力へ差し替えた config を一時生成する（fixture 本体は
  # golden テストと共有し、tier ごとに全文コピーしない）。
  sed "s#^sources = .*#sources = [\"${source}\"]#" "$BASE_CONFIG" > "$config"

  echo
  echo "== ${tier}（${source}） =="
  /usr/bin/time -l env RUST_LOG=info ./target/release/seiran build -c "$config" > "$log" 2>&1 || {
    cat "$log"
    return 1
  }
  grep -E "elapsed_ms|maximum resident set size|real[[:space:]]" "$log" || true
  # 出力先は fixture config の [output] 由来で全 tier 共通（sources だけを差し替えているため）。
  local output="target/test_document.pdf"
  if [ -f "$output" ]; then
    echo "output_size_bytes=$(wc -c < "$output" | tr -d ' ')"
  fi
}

measure small tests/text/itemize.sei
measure medium tests/text/perf/medium.sei
measure large tests/text/perf/large.sei
