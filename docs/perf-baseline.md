# 性能 baseline（issue #253 / #252 step1）

`build_pdf` を build driver / compiler core に分離するリファクタ（#252）に着手する前の、現状の
性能値を記録した baseline。CI で assert する性能テストではない（機種依存でフレークするため）。
step5〜7（`PublicationBuilder` / `encode_pdf` 書き換え）等、割り当て増加が疑われる step の前後で
`tools/perf-baseline.sh` を再実行し、この記録と目視比較する運用とする。

## 計測方法

```sh
tools/perf-baseline.sh
```

代表的な小・中・大規模文書について `cargo run --release -- build` を実行し、`build_pdf` が既に
出している段階別 `elapsed_ms` ログ（`RUST_LOG=info`）・`/usr/bin/time -l` の wall-clock 時間・
peak memory（maximum resident set size）・出力 PDF のファイルサイズを記録する。

- **small**: `tests/text/itemize.sei`（既存 golden 入力をそのまま使用）
- **medium** / **large**: `tests/text/perf/medium.sei` / `large.sei`
  （代表的な実文書がまだ存在しないため、既存フィクスチャの内容パターンを機械的に繰り返した合成
  文書。将来、代表的な実文書に差し替えてよい）

## 計測環境（この baseline の記録時点）

- 日付: 2026-07-20
- 機種: Apple M4（`Darwin 25.5.0`, arm64）
- メモリ: 24 GB
- `rustc 1.97.1`（release ビルド、`opt-level = 3` / `lto = true` / `codegen-units = 1`）

## baseline 数値

| tier | node_count | block_count | page_count | wall-clock (real) | peak memory | 出力 PDF サイズ |
| --- | --- | --- | --- | --- | --- | --- |
| small | 24 | 131 | 1 | 0.33 s | 255 MB | 80,420 B |
| medium | 90 | 240 | 3 | 0.05 s | 256 MB | 74,431 B |
| large | 720 | 1,920 | 18 | 0.22 s | 270 MB | 271,658 B |

peak memory は起動時の共有ライブラリ等を含む常駐セットサイズで、tier 間の差分（256 MB → 270 MB、
約 14 MB）が文書サイズに起因する増分の目安になる。small の wall-clock がやや大きいのはプロセス
起動・フォント読込のオーバーヘッドが相対的に効くため（`elapsed_ms` の段階内訳を参照）。

### 段階別 `elapsed_ms`（代表値、1 回計測）

| 段階 | small | medium | large |
| --- | --- | --- | --- |
| フォント読込 | 18 | 10 | 9 |
| 全ソースのパース | 0 | 0 | 1 |
| 文献引用の CSL 整形 | 0 | 0 | 0 |
| フォント検証 | 0 | 0 | 0 |
| Document IR → LayoutNode | 0 | 0 | 0 |
| 本文ブロックの構築（build_blocks） | 2 | 1 | 7 |
| 画像サイズの確定 | 0 | 0 | 0 |
| 本文のページ分割（break_pages） | 9 | 23 | 174 |
| PDF の描画 | 20 | 15 | 30 |

`break_pages` が文書サイズに対して非線形に増える（3 → 18 ページで 23ms → 174ms）のが現状の
プロファイル上の特徴。今回のリファクタ（#252）は組版アルゴリズム自体を変えない前提なので、この
傾向が step 前後で大きく変わらないことを確認する基準にする。

## 再計測時の注意

- 生 log は `tools/perf-baseline.sh` の一時ディレクトリに書かれ、実行後に破棄される。値をこの
  ファイルへ転記するのは手動（perf は機種・負荷状況に依存するため、自動転記・自動 assert はしない）
- 1 回計測のばらつきを避けたい場合は複数回実行して中央値を見る（本 baseline は各 tier 1 回計測）
