# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースの CLI ツールです。Rust Edition 2024、Cargo workspace 構成（resolver = "3"）。

## 言語設計原則

LaTeX の主要機能を組み込みで提供しつつ、曖昧さを排除する汎用組版言語（マクロ・パッケージ機構・LaTeX 互換層なし）。
目的は 3 つ — **G1 一意に読める**（字面だけで構造が一意）/ **G2 効果が閉じる**（効果範囲が構文から見える）/
**G3 内容が見た目から独立**（ソースは内容のみ）。

新コマンド・新環境・新オプション・新スタイルフィールドの設計は以下の原則に整合させる。
各原則の導出・根拠・適合例・過去の判断事例は **`docs/language-design.md`** に集約してあり、
原則と欲しい機能が衝突したときは例外を継ぎ足さず、目的に照らして原則側の改訂も検討する。

| #   | 原則（要約）                                                                    | 目的   |
| --- | ------------------------------------------------------------------------------- | ------ |
| P1  | トークン規則は固定 — カテゴリコードなし、コメント `//`、特殊文字はエスケープ必須 | G1     |
| P2  | 必須引数は `{}` で明示・個数順序固定、コマンド名は英数字                         | G1     |
| P3  | オプションは `[key=value]` の名前付きのみ（位置依存オプション禁止）              | G1     |
| P4  | `{}` は「引数境界」と「数式内グループ」のみ、裸の `{...}` は構文エラー           | G1     |
| P5  | 数式はインライン `$...$` のみ、ディスプレイは数式環境のみ（`$$` `\[` 不採用）    | G1     |
| P6  | 未知は拒否 — 未知のコマンド・環境・キーはエラー、静かな無視なし                  | G1     |
| P7  | 効果範囲は構文で閉じる — 引数・環境本体・構造上固定の単位のいずれか              | G2     |
| P8  | マクロ置換なし — 将来の `\define` は固定引数の新コマンド定義                     | G1, G2 |
| P9  | プログラミング機能なし — 計算・条件分岐・ループを提供しない                      | G1, G2 |
| P10 | 種類の既定は style.toml / config.toml、個別要素の設定はソースのオプション        | G3     |

**P10 の判定テスト**: 「これは 1 要素の設定か、種類の既定か？」個 → ソース（`\image[width=5cm]` 可）。
種類 → style.toml（見た目）/ config.toml（物理・実体・メタ）。プリアンブル禁止・クラス概念の不採用は P10 から導出される。

## コマンド

```sh
cargo build                                                # デバッグビルド
cargo build --release                                      # リリースビルド（LTO 有効）
cargo run -- build [-c <config_path>]                      # 設定ファイルの sources から PDF を生成
cargo run -- variation-axes <font> [-f <font_index>]       # バリアブルフォント軸情報を表示
cargo run -- ttc-names <ttc_file>                          # TTC ファイル内のフォント名一覧を表示
cargo run -- script-langs <font> [-f <font_index>]         # サポートされるスクリプト / 言語を表示
cargo +nightly fmt                                         # フォーマット（nightly 必須）
cargo clippy                                               # リント
cargo test                                                 # テスト実行
cargo test -p <crate_name>                                 # 特定クレートのテスト実行
```

`cargo fmt` は **nightly toolchain が必須**です。`rustfmt.toml` で `unstable_features = true`（`group_imports = "StdExternalCrate"` / `imports_granularity = "Crate"` / `format_macro_bodies` 等）を有効化しているためです。`build` サブコマンドの `-c` / `--config` を省略した場合は `./config/config.toml` が使用されます。

## アーキテクチャ

### データフロー

```text
CLI 引数パース → TOML 設定読込（メイン設定 / スタイル / 参照定義）
  → 字句解析・構文解析（syntax: Lexer → Parser → CST）
  → 評価（parser: CST → Document IR（document::DocNode））
  → 文献引用整形（citation: \cite を CSL 整形＝hayagriva で採番し、書誌を本文末尾に追加）
  → ローワリング（lowering: DocNode → LayoutNode）→ フォント読込・検証
  → (a) build_blocks（layout: LayoutNode → Vec<Block>。シェーピング + 計測 + break 注入）
  → (prepass) resolve_images（pdf_gen: 画像の自然寸法から width/height を確定）
  → (c+d) break_pages（hlist: 行分割 + 縦組版 → Vec<Page>。フォント非依存の純粋パス）
  → (e) render_pages（pdf_gen: 確定座標の描画のみ。krilla がフォントサブセット化を内部実施）
  → ファイル出力
```

box は (a) で width/height/depth を 1 回だけ計測して保持し、以降のパスはフォントに触れない。
本文の自動行折り返しは Knuth–Plass（段落全体最適。`hlist::break_lines`。貪欲法 first-fit も
`GreedyBreaker` として併存）で、既定は両端揃え（`[text]` の `alignment`。glue の伸縮で行幅を調整）。
分割可能点は ICU `LineSegmenter`（UAX #14）により和欧同時に求め（`hlist::break_opportunities`）、
欧文語中は discretionary ハイフネーション（`hlist::hyphenation`）を併用する。
縦組版（`break_pages`）も glue/penalty モデルで、widow/orphan・keep-with-next・
下端揃え（flush_bottom）を penalty と glue 伸縮で制御する。
数式は `HBoxContent::Atom`（絶対 dx/dy の閉じた箱）として行分割をまたがない。

### クレート依存関係

```text
types （依存なし — 共通型の基盤。Length / HeadingLevel / TableColumn / ColumnAlign / ColumnWidth もここ）
  ↑ read_config, read_style, document, font, hlist, lowering, layout, parser, citation, pdf_gen, seiran

read_config （types を使用）
  ↑ font, pdf_gen, seiran

read_style （types を使用）
  ↑ citation, lowering, pdf_gen, seiran

read_references （workspace クレートに依存しない独立クレート）
  ↑ citation, seiran

syntax （bumpalo アリーナ上に CST を構築。workspace クレートに依存しない）
  ↑ parser

document （types のみに依存。Document IR の共有契約クレート）
  ↑ parser, citation, lowering, seiran

parser （syntax の CST を Document IR（document）に変換。採番・書式化は行わず lowering に委ねる）
  ↑ seiran

citation （document, read_references, read_style, types に依存。hayagriva / citationberg で CSL 整形）
  ↑ seiran

hlist （types, icu, hypher のみに依存。フォント・krilla 非依存の純粋組版パスとコア型）
  ↑ layout, pdf_gen, seiran

font （types, read_config に依存。read-fonts / harfrust / rayon を使用）
  ↑ layout, pdf_gen, seiran

lowering （document, read_style, types に依存。フォント非依存の論理変換層。採番・`\ref` 解決も担う）
  ↑ layout, seiran

layout （font, hlist, lowering, types に依存。icu でスクリプト判定）
  ↑ seiran

pdf_gen （font, hlist, read_config, read_style, types に依存。krilla / krilla-svg で PDF を生成）
  ↑ seiran

cli （clap のみに依存）
  ↑ seiran

subcommand （miette / read-fonts / thiserror / tracing のみに依存。workspace クレート非依存）
  ↑ seiran

seiran （エントリーポイント。全クレートを統合してパイプラインを実行）
```

### 各クレートの責務

ナビゲーション用の 1 行要約。サブモジュール構成・内部設計・データ構造などの詳細は
`docs/architecture.md` に集約しているので、特定クレートを触る前にそちらを参照する。

| クレート          | 責務（要約）                                                                          |
| ----------------- | ------------------------------------------------------------------------------------- |
| `types`           | 全クレート共通型（`FontType` / `FontKind` / `FontMap` / `Length` / `HeadingLevel` / `TableColumn` 等） |
| `cli`             | clap derive の CLI 引数定義（`Build` / `VariationAxes` / `TtcNames` / `ScriptLangs`）  |
| `read_config`     | `config.toml` の読込・`garde` バリデーション                                            |
| `read_style`      | `style.toml` の読込・デフォルトマージ・`garde` バリデーション。単層 `Style` を提供       |
| `read_references` | `references.toml` / `.json` の読込（CSL 文献情報、拡張子で判別）                         |
| `syntax`          | 字句・構文解析（`lexer` → `parser`）、bumpalo アリーナ上にロスレス CST を構築           |
| `document`        | Document IR の型定義（`parser` 生産・`lowering` 消費の共有契約クレート）                 |
| `parser`          | CST → Document IR の評価変換。コマンド / 環境を phf レジストリでディスパッチ（採番なし）  |
| `citation`        | `\cite` の CSL 整形（採番 + 書誌生成、hayagriva / citationberg）                        |
| `hlist`           | フォント非依存のコア型 + 純粋組版パス（(b) break_opportunities / (c) break_lines / (d) break_pages） |
| `font`            | フォント読込・シェーピング・検証・バリアブルフォント（read-fonts / harfrust / rayon）   |
| `lowering`        | DocNode → LayoutNode の論理変換（フォント非依存）。採番・`\ref` 解決（pass1/pass2）も担う |
| `layout`          | (a) build_blocks: LayoutNode → `Vec<Block>`（シェーピング + 計測 + break 注入）。running でヘッダ / フッタ配置 |
| `pdf_gen`         | (e) render_pages: 確定座標を描画 + resolve_images prepass。krilla で PDF 生成           |
| `subcommand`      | `variation-axes` / `ttc-names` / `script-langs` 実装（read-fonts 直接使用）             |
| `seiran`          | main エントリ。全クレート統合・パイプライン実行                                          |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない）
2. **インデント**: 2 スペース（`rustfmt.toml` で設定済み）
3. **最大行幅**: 120 文字
4. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`。型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい
5. **ドキュメントコメント**: すべてのモジュール・構造体・関数に **日本語** で記述
6. **フォーマッタ**: `cargo +nightly fmt` を使用（`unstable_features = true`）

### モジュール構成

- **`mod.rs` を使わない**: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `font::shaper` / `syntax::span` / `types::length` の garde バリデータ / `read_config::test_support`）。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### エラーハンドリング・バリデーション

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語、`code` は `<crate>::<category>::<name>` で階層化）。`miette::Result<T>` は `main` / 上位パイプライン関数のみで使う
- 設定値検証は `garde` の `#[derive(Validate)]` で宣言的に書き、違反は `MultipleValidationErrors` に集約して 1 度に報告する
- バリアント設計・`#[label]` / `NamedSource` によるソース位置付与・`#[related]` 集約の制約・garde パターンの詳細は `error-handling` skill を参照する。新しいエラー型の定義・バリアント追加・バリデーション追加の際は必ず参照すること

### Clippy

`clippy::all` が deny、`pedantic` が warn。`needless_return` / `cast_possible_truncation` / `similar_names` / `too_many_lines` は allow。

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei` ファイル群）、フォント: リポジトリ直下の `fonts/`
- AAA パターン（Arrange / Act / Assert）で記述する
- **golden テスト・組版変更の検証**: レイアウトダンプ golden（`crates/seiran/src/build_pdf/golden.rs`）と PDF バイト比較の使い分け、前提資産の取得（初回は `tools/fetch-test-assets.sh` を 1 度実行）、golden の再生成、新機能へのテスト追加は `verify-typesetting` skill を参照する

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                             |
| ----------------------------------- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ・余白、`[pdf].show_bookmarks`（しおり出力）、`[image]`（画像 DPI / downsample）、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語             |
| `style.toml`                        | **見た目**                 | 見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、段組み数、参照リンク色、フロート挙動デフォルト |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                   |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）
- **値の基本書式**: 長さ（`Length`）は単位付き文字列 `"12pt"` / `"5mm"`（素の数値は不可）、色（`Color`）は `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問、`[r, g, b]` 配列は不可）
- **style.toml の詳細スキーマ**（キャプションと番号 3 系統・見出し 2 レイヤーマージ・カウンタ固定 9 種・`[math.script]` / `[math.block]`・`[page]` の `flush_bottom` 等）は `docs/architecture.md` の read_style 節を参照

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`

## issue / PR 運用

issue・PR・branch・commit・ラベル・sub-issue の運用規約は `issue-pr-ops` skill に集約。
GitHub 上で issue / PR を作る・編集する、branch を切る、commit メッセージや merge 方法を決める、
ラベルや epic / sub-issue の親子関係を判断する際は、その skill を参照すること。
クレート構成・パイプライン・設定スキーマ・CLI に触れる PR を仕上げる際は
`docs-sync` skill のチェックリストでドキュメント更新漏れを確認すること。
