# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースの CLI ツールです。Rust Edition 2024、Cargo workspace 構成。

## コマンド

```sh
cargo build                                      # デバッグビルド
cargo build --release                            # リリースビルド（LTO 有効）
cargo run build <text_file>                      # テキストファイルから PDF を生成
cargo run variation-axes <font> [--font-index N] # バリアブルフォント軸情報を表示
cargo run ttc-names <ttc_file>                   # TTC ファイル内のフォント名一覧を表示
cargo run script-langs <font> [--font-index N]   # サポートされるスクリプト / 言語を表示
cargo fmt                                        # フォーマット
cargo clippy                                     # リント
cargo test                                       # テスト実行
cargo test -p <crate_name>                       # 特定クレートのテスト実行
```

## アーキテクチャ

### データフロー

```text
CLI引数パース → TOML設定読込（メイン設定 / スタイル / 参照定義）
  → テキストパース（Lexer → Parser → Evaluator → DocNode）
  → ローワリング（DocNode → LayoutNode）→ フォント読込/検証
  → テキストシェーピング → レイアウトエンジン（LayoutNode → Item）
  → フォントサブセット化 → PDF生成 → ファイル出力
```

### クレート依存関係

```text
types（依存なし — 全クレートの基盤）
  ↑
read_config / read_style / read_references
  ↑
font（allsorts, harfrust, rayon による並列処理）
  ↑
parser（Lexer → Parser → Evaluator → DocNode）
  ↑
layout（DocNode → LayoutNode → Item）
  ↑
pdf_gen（pdf-writer / krilla による PDF バイナリ生成）
  ↑
seiran（エントリーポイント、パイプライン全体の統合）

cli（clap のみ依存）
subcommand（cli, font, read_config, types）
```

### 各クレートの責務

| クレート | 責務 |
|---|---|
| `types` | `FontType`, `FontKind`, `FontMap` など全クレート共通型 |
| `cli` | clap derive による CLI 引数定義 |
| `read_config` | `config/config.toml` の読み込み・バリデーション（`garde` 派生 + `MultipleValidationErrors` 集約） |
| `read_style` | `config/style.toml` の読み込み（figment によるデフォルト値マージ、`garde` 派生によるバリデーション） |
| `read_references` | `config/references.toml` の読み込み（CSL 文献情報） |
| `font` | フォント読込・シェーピング・サブセット化・バリアブルフォント対応 |
| `parser` | テキストファイルのレキサー・パーサー・エバリュエーター |
| `layout` | DocNode → LayoutNode → Item へのローワリングとレイアウト計算 |
| `pdf_gen` | PDF バイナリ生成 |
| `seiran` | main、全クレートのオーケストレーション |
| `subcommand` | variation-axes / ttc-names / script-langs サブコマンド実装 |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない）
2. **インデント**: 2 スペース（`rustfmt.toml` で設定済み）
3. **最大行幅**: 120 文字
4. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化
5. **ドキュメントコメント**: すべてのモジュール・構造体・関数に **日本語** で記述

### エラーハンドリング

- `thiserror` + `miette` によるエラー型を定義する
- `#[derive(thiserror::Error, Debug, miette::Diagnostic)]` を常に併用する
- `#[diagnostic(code(...), help("..."))]` でユーザーフレンドリーな診断情報を付与する
- `#[source]` 属性でエラーチェーンを形成し、`?` 演算子で伝播する
- `Box<dyn std::error::Error>` は `main` のみ許可

```rust
#[derive(thiserror::Error, Debug, miette::Diagnostic)]
pub enum MyError {
  #[error("Failed to read file: {path}")]
  #[diagnostic(
    code(my_error::io),
    help("ファイルのパスと読み取り権限を確認してください")
  )]
  Io {
    path: String,
    #[source]
    source: std::io::Error,
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
