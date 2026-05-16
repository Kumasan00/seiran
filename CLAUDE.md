# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースの CLI ツールです。Rust Edition 2024、Cargo workspace 構成（resolver = "3"）。

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
  → 評価（parser: CST → Document IR（DocNode））
  → ローワリング（layout: DocNode → LayoutNode）→ フォント読込・検証
  → テキストシェーピング → レイアウトエンジン（LayoutNode → Item）
  → フォントサブセット化 → PDF 生成 → ファイル出力
```

### クレート依存関係

```text
types （依存なし — 共通型の基盤）
  ↑ read_config, font, layout, pdf_gen, seiran

read_config （types を使用）
  ↑ font, pdf_gen, seiran

read_style / read_references （workspace クレートに依存しない独立クレート）
  ↑ read_style: parser, layout, pdf_gen, seiran
  ↑ read_references: seiran

syntax （bumpalo アリーナ上に CST を構築。workspace クレートに依存しない）
  ↑ parser

parser （syntax の CST を Document IR に変換。read_style に依存）
  ↑ layout, seiran

font （types, read_config に依存。allsorts / harfrust / rayon を使用）
  ↑ layout, pdf_gen, seiran

layout （font, parser, read_style, types に依存）
  ↑ pdf_gen, seiran

pdf_gen （font, layout, read_config, read_style, types に依存。pdf-writer / krilla で PDF を生成）
  ↑ seiran

cli （clap のみに依存）
  ↑ seiran

subcommand （miette / read-fonts / thiserror / tracing のみに依存。workspace クレート非依存）
  ↑ seiran

seiran （エントリーポイント。全クレートを統合してパイプラインを実行）
```

### 各クレートの責務

| クレート | 責務 |
|---|---|
| `types` | `FontType`, `FontKind`, `FontMap` など全クレート共通型 |
| `cli` | clap derive による CLI 引数定義（`Build` / `VariationAxes` / `TtcNames` / `ScriptLangs`） |
| `read_config` | `config/config.toml` の読み込み・バリデーション（`garde` 派生 + `MultipleValidationErrors` 集約） |
| `read_style` | `config/style.toml` の読み込み（figment によるデフォルト値マージ、`garde` 派生によるバリデーション） |
| `read_references` | `config/references.toml` の読み込み（CSL 文献情報） |
| `syntax` | 字句解析・構文解析（`lexer` → `parser`）、`bumpalo::Bump` アリーナ上にロスレスな CST（`green::GreenNode`）を構築。型付きビュー（`ast::CommandView`, `ast::EnvironmentView`）を提供 |
| `parser` | `syntax` の生成した CST を走査し、Document IR（`document::DocNode`, `InlineNode`, `MathNode` 等）に評価変換。`evaluator/` 配下にコマンド・環境・カウンタ・インライン要素のサブモジュール |
| `font` | フォント読込・シェーピング・サブセット化・バリアブルフォント対応（`shaper.rs`, `validate_font.rs`） |
| `layout` | DocNode → LayoutNode へのローワリング（`lowering.rs`）、LayoutNode → Item のレイアウト計算（`layout_engine.rs`） |
| `pdf_gen` | krilla / krilla-svg / pdf-writer による PDF バイナリ生成 |
| `subcommand` | `variation-axes` / `ttc-names` / `script-langs` サブコマンド実装。`read-fonts` を直接使用（font クレート非依存） |
| `seiran` | `main` エントリーポイント、全クレートのオーケストレーション、`tracing-subscriber` の初期化 |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない）
2. **インデント**: 2 スペース（`rustfmt.toml` で設定済み）
3. **最大行幅**: 120 文字
4. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`
5. **ドキュメントコメント**: すべてのモジュール・構造体・関数に **日本語** で記述
6. **フォーマッタ**: `cargo +nightly fmt` を使用（`unstable_features = true`）

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

- テスト用入力: `tests/text/text.txt`、テスト用フォント: `tests/fonts/`
- AAA パターン（Arrange / Act / Assert）で記述する

## 設定ファイル

- `config/config.toml` — PDF 出力サイズ・余白・フォント設定（19 種別）
- `config/style.toml` — 見出しフォントサイズ・余白（figment でデフォルト値マージ）
- `config/references.toml` — CSL ベース文献情報

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`
