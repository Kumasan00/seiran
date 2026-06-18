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
- **E1. 宣言型コマンド廃止** — `\bfseries` のような状態変更コマンドは作らない。装飾は引数型コマンドか環境のみ
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

| クレート          | 責務                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `types`           | `FontType`, `FontKind`, `FontMap`, `Length`, `HeadingLevel`, `TableColumn` など全クレート共通型                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `cli`             | clap derive による CLI 引数定義（`Build` / `VariationAxes` / `TtcNames` / `ScriptLangs`）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `read_config`     | `config/config.toml` の読み込み・バリデーション（`garde` 派生 + `MultipleValidationErrors` 集約）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `read_style`      | `config/style.toml` の読み込み（`serde(default)` でデフォルト値マージ、`garde` 派生によるバリデーション）。単層の `Style` 構造体が lowering/pdf_gen の読むフィールド（`font_size` / `line_height_factor` / `background_color` / `heading` / `text` / `list` / `table` / `figure` / `equation` / `math` / `counters` / `page_numbering` / `header` / `footer` / `reference` / `hyperref` / `title_page` / `toc`）をトップレベルに保持する。各サブスタイル型（`CaptionStyle` 等）はクレート直下のモジュール（`caption` / `heading` / `figure` 等）に置き、トップレベル（`read_style::FigureStyle` 等）で再エクスポートする。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML パース時に弾く。`style.reference` は `citation` が参照（`title` は書誌見出し文字列、`csl_path` は CSL スタイル `.csl` のパス＝採番方式・書誌体裁、`locale_path` は CSL ロケール XML のパスで内蔵ロケールを上書き・補強）。`header` / `footer` は共通の `RunningContentStyle`（左中右スロット・トークン `{page}` `{pages}` `{title}` `{author}` `{date}`）                                             |
| `read_references` | `config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式判別）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `syntax`          | 字句解析・構文解析（`lexer` → `parser`）、`bumpalo::Bump` アリーナ上にロスレスな CST（`green::GreenNode`）を構築。型付きビュー（`ast::CommandView`, `ast::EnvironmentView`）を提供                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `document`        | Document IR の型定義（`Document` / `DocNode` / `InlineNode` / `MathNode` / `CaptionPosition` / `ListItem` / `TableRow` / `TableCell`）。`parser`（生産者）と `lowering`（消費者）双方が依存する共有契約クレート。セマンティック情報のみ保持し、物理レイアウト情報は持たない（`block` / `caption` / `inline` / `list` / `math` / `table` サブモジュール）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `parser`          | `syntax` の生成した CST を走査し、Document IR（`document` クレートの `DocNode` 等）に評価変換。`evaluator/` 配下にコマンド（`control` / `headline` / `inline` / `ref_`）・環境（`body_scan` / `caption` / `equation` / `figure` / `itemize` / `table`）・カウンタ・インライン要素・数式・オプション引数のサブモジュール                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `citation`        | `\cite` の CSL 整形ステージ（parser の後・lowering の前）。`process_citations` が `InlineNode::Cite` をドキュメント順に走査し、`hayagriva`（`archive` feature 内蔵ロケール + `citationberg` で `.csl` 解析）で引用ラベルを採番（`[1][2]…`）して `label` を確定、引用された文献の書誌（References 見出し + 段落群）を本文末尾に追加。CSL スタイルは `style.reference.csl_path` の `.csl` を読む（引用があるのに未設定なら `MissingCslPath` エラー）。ロケールは `load_locales` が `style.reference.locale_path` の CSL ロケール XML を内蔵ロケールの前段に重ねて採番に渡す（同一言語コードはカスタム優先）。`bridge`（`read_references::Reference` → `hayagriva::Entry` 変換）/ `render`（`BibliographyDriver` 駆動・`ElemChildren` → `InlineNode` 変換）サブモジュール構成。初版は引用/書誌ともプレーン文字列（斜体等は段階対応） |
| `hlist`           | フォント非依存のコア型（`HItem` / `HBox` / `Atom` / `Block` / `Line` / `Page` / `GlyphRun` / `TableBox`）と純粋組版パス: (b) `break_opportunities`（ICU UAX #14）、(c) `break_lines`（`LineBreaker` / `GreedyBreaker`）、(d) `break_pages`（ベースライン送り・改ページ・表分割・`PageGeometry`）。表の列幅・行高の純粋計測もここ                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `font`            | フォント読込・シェーピング・検証・バリアブルフォント対応（`shaper.rs`, `validate_font.rs`）、`FontMetrics`（upem / ascender / descender の一元化）。`read-fonts` / `harfrust` / `rayon` を使用                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `lowering`        | DocNode → LayoutNode への論理変換層（`lib.rs` + `figure` / `float` / `heading` / `inline` / `list` / `math` / `paragraph` / `table` / `template` サブモジュール）。`LayoutNode` / `TextStyle` / `TableLayout` の型定義は `layout_node` に置く。フォント・シェーピング非依存。縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す（残る `LineBreak` は段落内 `\\` 由来のみ）                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `layout`          | (a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストのスクリプト分割・シェーピング・計測、break 注入（シェーピング後に `GlyphRun` を ICU の分割可能位置で分割。数式は分割しない）、`Raise` ツリーの `Atom` 化。`icu` でスクリプト判定、`font` のシェーパーと `FontMetrics` を利用。`running` サブモジュールの `build_running_content` は `break_pages` 後（ページ数確定後）にヘッダー・フッターをトークン展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する                                                                                                                                                                                                                                                                                               |
| `pdf_gen`         | (e) `render_pages`（`render`）: 確定座標の `Vec<Page>` を描画するだけ（レイアウト判断ゼロ）。`resolve_images` prepass（画像サイズ確定、`image`）もここ。`krilla` / `krilla-svg` による PDF バイナリ生成（フォントサブセット化は krilla が内部で実施）。`error` / `font` / `image` / `metadata` / `render` サブモジュール構成                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `subcommand`      | `variation-axes` / `ttc-names` / `script-langs` サブコマンド実装。`read-fonts` を直接使用（font クレート非依存）                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `seiran`          | `main` エントリーポイント、全クレートのオーケストレーション、`tracing-subscriber` の初期化                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |

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

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                             |
| ----------------------------------- | -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ・余白、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語             |
| `style.toml`                        | **見た目**                 | 見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、段組み数、参照リンク色、フロート挙動デフォルト |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                   |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）

#### `style.toml` の主要なスキーマ

- **長さ値（`Length`）**: フォントサイズ・余白等は単位付き文字列 `"12pt"` または `"5mm"` で指定（素の数値は不可）
- **色（`Color`）**: `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問）。`[r, g, b]` 配列形式は不可
- **キャプション**: figure / table は共通の `CaptionStyle { format, font_size }` を `caption` フィールドに持つ。配置は図・表ともソース上の `\caption` の出現位置（本体より前なら Top、後なら Bottom）で決まり、スタイル側では指定しない。equation は `number_format` / `number_side` を維持
- **見出し（2 レイヤーマージ）**: `default_for_level()` (Rust) → `[heading.<level>]`（レベル別差分）の順に重畳。`[heading]` 直下にスカラーは書けない（テーブル形式のみ）
- **カウンタ（`CounterStyle`）**: `[counters.<name>]` の `<name>` は固定 9 種（`part` / `chapter` / `section` / `subsection` / `paragraph` / `subparagraph` / `table` / `figure` / `equation`）のみ。各エントリは `display_name` / `format` / `number_style` / `ref_format` / `resets` を持ち、未知のカウンタ名は `deny_unknown_fields` で拒否
- **数式パラメータ（`MathScriptStyle`）**: 上付き / 下付きの倍率・シフト等。将来 OpenType MATH テーブルから自動取得する想定で、現状は手動指定（`Option<MathScriptStyle>` 化の余地を残す）

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`
