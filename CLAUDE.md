# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースの CLI ツールです。Rust Edition 2024、Cargo workspace 構成（resolver = "3"）。

## 言語設計原則

LaTeX の主要機能をデフォルトで提供しつつ、LaTeX の曖昧さを排除する方針。新コマンド・新環境を設計する際は以下に整合させる。

- **A. 引数は必ず `{}` で明示** — `\section Title` 不可、`\section{Title}` のみ
- **B. コマンド名** — 英数字始まり、続く文字も英数字。英数字以外で終端
- **C. マクロ置換は原則不可** — `\newcommand` 相当はデフォルトで提供しない（拡張は将来の `\define` メタコマンド + `.seiran` ファイル）
- **D. カテゴリコードなし** — トークン規則は固定
- **E1. 宣言型コマンドは原則避ける（曖昧さの排除が目的・例外あり）** — 目的は LaTeX の曖昧さ排除と分かりやすさであり、宣言型コマンドを避けるのはそのための既定手段（目的そのものではない）。`\bfseries` のように「以降ずっと効き、いつ終わるか不明瞭」な状態変更コマンドは曖昧さを生むため作らない（装飾は引数型コマンドか環境で表す）。ただし効果範囲が構造的に閉じ、置ける位置も一意で曖昧さがないなら宣言型でも認める（例: `align` / `gather` の行末マーカー `\notag` — 効果はその行のみ・位置は行末固定）
- **E2. 裸の `{...}` は構文エラー** — `{}` の意味は「コマンドの引数の境界」と「数式内のグループ化」だけ
- **F. プログラミング機能なし** — 計算・条件分岐は未実装。コメントは `//`（将来 `%` を剰余演算子に確保するため）
- **G. 数式モード境界の単純化** — インライン `$...$` のみ、ディスプレイは数式環境のみ。`$$...$$` `\[...\]` は不採用

**コンテンツとプレゼンテーションの完全分離**: ソースは本文のみ、メタデータ（title/author/date）・物理設定は config.toml、見た目は style.toml に集約。プリアンブル相当はソースに書けない。`\documentclass` 相当のクラス概念も導入しない。

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
本文の自動行折り返しは貪欲法（first-fit）・左揃え（ragged-right）で、分割可能点は
ICU `LineSegmenter`（UAX #14）により和欧同時に求める（`hlist::break_opportunities`）。
数式は `HBoxContent::Atom`（絶対 dx/dy の閉じた箱）として行分割をまたがない。

### クレート依存関係

```text
types （依存なし — 共通型の基盤。Length / HeadingLevel / TableColumn / ColumnAlign / ColumnWidth もここ）
  ↑ read_config, read_style, document, font, hlist, lowering, layout, parser, pdf_gen, seiran

read_config （types を使用）
  ↑ font, pdf_gen, seiran

read_style （types を使用）
  ↑ parser, lowering, pdf_gen, seiran

read_references （workspace クレートに依存しない独立クレート）
  ↑ citation, seiran

syntax （bumpalo アリーナ上に CST を構築。workspace クレートに依存しない）
  ↑ parser

document （types のみに依存。Document IR の共有契約クレート）
  ↑ parser, lowering, seiran

parser （syntax の CST を Document IR（document）に変換。read_style に依存）
  ↑ seiran

citation （document, read_references, read_style, types に依存。hayagriva / citationberg で CSL 整形）
  ↑ seiran

hlist （types, icu のみに依存。フォント・krilla 非依存の純粋組版パスとコア型）
  ↑ layout, pdf_gen, seiran

font （types, read_config に依存。read-fonts / harfrust / rayon を使用）
  ↑ layout, pdf_gen, seiran

lowering （document, read_style, types に依存。フォント非依存の論理変換層）
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
| `parser`          | CST → Document IR の評価変換。コマンド / 環境を phf レジストリでディスパッチ             |
| `citation`        | `\cite` の CSL 整形（採番 + 書誌生成、hayagriva / citationberg）                        |
| `hlist`           | フォント非依存のコア型 + 純粋組版パス（(b) break_opportunities / (c) break_lines / (d) break_pages） |
| `font`            | フォント読込・シェーピング・検証・バリアブルフォント（read-fonts / harfrust / rayon）   |
| `lowering`        | DocNode → LayoutNode の論理変換（フォント非依存）                                       |
| `layout`          | (a) build_blocks: LayoutNode → `Vec<Block>`（シェーピング + 計測 + break 注入）。running でヘッダ / フッタ配置 |
| `pdf_gen`         | (e) render_pages: 確定座標を描画 + resolve_images prepass。krilla で PDF 生成           |
| `subcommand`      | `variation-axes` / `ttc-names` / `script-langs` 実装（read-fonts 直接使用）             |
| `seiran`          | main エントリ。全クレート統合・パイプライン実行                                          |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない）
2. **インデント**: 2 スペース（`rustfmt.toml` で設定済み）
3. **最大行幅**: 120 文字
4. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`
5. **ドキュメントコメント**: すべてのモジュール・構造体・関数に **日本語** で記述
6. **フォーマッタ**: `cargo +nightly fmt` を使用（`unstable_features = true`）

### モジュール構成

- **`mod.rs` を使わない**: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### エラーハンドリング

- 各クレートの `lib.rs`（または該当モジュール）に `thiserror::Error` + `miette::Diagnostic` 派生のエラー列挙型を定義する。`#[derive(Debug, Error, Diagnostic)]` を常に併用する
- バリアントごとに `#[error("...")]`（メッセージ、日本語）と `#[diagnostic(code(<crate>::<category>::<name>), help("..."))]` を付与する。`code` は `<crate>::<category>` を接頭辞にコロン区切りで階層化する（例: `config::validation::field`, `parser::eval::unknown_command`）
- 外部エラーを巻き取る場合は `#[source] source: ExternalError` フィールドで chain を形成し、`?` 演算子で伝播する。`map_err` でメッセージのコンテキスト（ファイルパス等）を付与する
- ソーステキストに紐づくエラー（パース・評価系）は `#[label("...")] span: miette::SourceSpan` を持たせる。エントリポイント（例: `parser::parse_source`）では `miette::NamedSource` を保持するラッパー enum（例: `ParseSourceError`）を返し、変種に `#[source_code] src: NamedSource<String>` と内側のエラーへの `#[source] #[diagnostic_source] error: InnerError` を持たせて Diagnostic を伝播する。これにより `#[related]` 集約時もソースコード付きの label がレンダリングされる
- 複数エラーを 1 度にまとめて報告する場合は `#[related] errors: Vec<...>` を持つ集約バリアント（例: `MultipleValidationErrors`）を作る。`#[related]` の要素は **`Diagnostic` 実装が必須**であり、`miette::Report` は実装しないため、`Report` を直接ベクタに詰めることはできない。クレート固有のエラー型（例: `ParseSourceError`）を返すことでこの問題を回避する
- 関数のシグネチャは原則 **クレート固有のエラー型を返す**（例: `Result<Config, ReadConfigError>`, `Result<Vec<DocNode>, ParseSourceError>`）。`miette::Result<T>` は `main` や上位パイプライン関数（`build_pdf`, `layout_engine` 等）でのみ使い、ライブラリ的な公開 API では避ける。`Report` は `Diagnostic` を実装しないので、`#[related]` で集約される可能性のあるエラーは具体型で返すこと
- 外部クレートの `Result<T, E>` を `miette::Result<T>` に持ち上げる際は `miette::IntoDiagnostic` の `.into_diagnostic()?` を使用する
- `main` は `miette::Result<()>` を返す（`Box<dyn std::error::Error>` は使わない）。`miette` の `fancy` feature により色付き診断が出力される

```rust
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum MyError {
  /// I/O 失敗: 外部エラーを #[source] で連鎖
  #[error("ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(my_crate::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },

  /// ソース位置付きエラー: #[label] + SourceSpan、NamedSource は呼び出し側で添付
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(code(my_crate::eval::unknown_command), help("コマンド名のスペルを確認してください。"))]
  UnknownCommand {
    name: String,
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 集約バリアント: 検出した違反を 1 度に報告
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(my_crate::multiple_validation_errors))]
  MultipleValidationErrors {
    #[related]
    errors: Vec<ValidationError>,
  },
}
```

### バリデーション

設定ファイルの値検証は `garde` の `#[derive(Validate)]` + フィールド属性（`range` / `length` / `ascii` / `dive` / `custom`）で宣言的に記述する。複雑な相互制約は `custom` バリデーターで補い、検出した不正は `ValidationError::Field { path, message }` に変換して `MultipleValidationErrors { #[related] errors: Vec<ValidationError> }` に集約し、すべての違反を 1 度に報告する（`read_config` / `read_style` で同パターン）。

### Clippy

`clippy::all` が deny、`pedantic` が warn。`needless_return` / `cast_possible_truncation` / `similar_names` / `too_many_lines` は allow。

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `figure.sei` / `itemize.sei` / `table.sei` / `ref.sei`）、フォント: リポジトリ直下の `fonts/`
- AAA パターン（Arrange / Act / Assert）で記述する
- **golden テスト**（`crates/seiran/src/build_pdf/golden.rs`）: 入力はコミット済み fixture（`crates/seiran/tests/config/`）+ `tools/fetch-test-assets.sh` が `vendor/` へ取得するピン留め資産（フォント・CSL。SHA-256 検証、gitignore 対象・コミットしない）。初回はスクリプトを 1 度実行する。golden 再生成は `UPDATE_GOLDEN=1 cargo test -p seiran`。ユーザローカルの `config/` / `fonts/` はテストから参照しない

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                             |
| ----------------------------------- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ・余白、`[pdf].show_bookmarks`（しおり出力）、`[image]`（画像 DPI / downsample）、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語             |
| `style.toml`                        | **見た目**                 | 見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、段組み数、参照リンク色、フロート挙動デフォルト |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                   |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）

#### `style.toml` の主要なスキーマ

- **長さ値（`Length`）**: フォントサイズ・余白等は単位付き文字列 `"12pt"` または `"5mm"` で指定（素の数値は不可）
- **色（`Color`）**: `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問）。`[r, g, b]` 配列形式は不可
- **キャプション**: figure / table は共通の `CaptionStyle { format, font_size }` を `caption` フィールドに持つ。配置は図・表ともソース上の `\caption` の出現位置（本体より前なら Top、後なら Bottom）で決まり、スタイル側では指定しない。表示数式の番号体裁は `[math.block].tag_format` / `number_side`（番号 3 系統の **tag**＝式の横に出す。**number**＝`counters.equation.number_format`、**ref**＝`counters.equation.ref_format` とは別物）
- **見出し（2 レイヤーマージ）**: `default_for_level()` (Rust) → `[heading.<level>]`（レベル別差分）の順に重畳。`[heading]` 直下にスカラーは書けない（テーブル形式のみ）
- **カウンタ（`CounterStyle`）**: `[counters.<name>]` の `<name>` は固定 9 種（`part` / `chapter` / `section` / `subsection` / `paragraph` / `subparagraph` / `table` / `figure` / `equation`）のみ。各エントリは `display_name` / `number_format` / `number_style` / `ref_format` / `resets` を持ち、未知のカウンタ名は `deny_unknown_fields` で拒否
- **数式（`MathStyle`）**: `[math.script]`（`MathScriptStyle`＝上付き / 下付きの倍率・シフト等。インライン数式 `$...$` にも効く。将来 OpenType MATH テーブルから自動取得する想定で現状は手動指定）と `[math.block]`（`MathBlockStyle`＝表示数式ブロックのレイアウト。`tag_format` / `number_side` / `alignment` / `row_gap` / `column_gap` / `top_margin` / `bottom_margin`。全表示数式環境 equation / align / gather / split / multiline / cases / matrix が共有）の 2 副テーブルを束ねる。旧 `[equation]` テーブルは廃止（`[math.block]` に統合）

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`

## issue / PR 運用

issue・PR・branch・commit・ラベル・sub-issue の運用規約は `issue-pr-ops` skill に集約。
GitHub 上で issue / PR を作る・編集する、branch を切る、commit メッセージや merge 方法を決める、
ラベルや epic / sub-issue の親子関係を判断する際は、その skill を参照すること。
