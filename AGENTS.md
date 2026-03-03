# AGENTS.md — Seiran プロジェクト AI エージェントガイド

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースのコマンドラインツールです。

- **言語**: Rust (Edition 2024)
- **ライセンス**: MIT OR Apache-2.0
- **リポジトリ**: <https://github.com/Kumasan00/seiran>

---

## アーキテクチャ

### データフロー

```text
CLI引数パース → TOML設定読込（メイン設定 / スタイル / 参照定義）
  → テキストパース(Lexer→Parser→Evaluator→DocNode)
  → ローワリング(DocNode→LayoutNode) → フォント読込/検証
  → テキストシェーピング → レイアウトエンジン(LayoutNode→Item)
  → フォントサブセット化 → PDF生成 → ファイル出力
```

### クレート依存関係

```text
types (依存なし — 全クレートの基盤)
  ↑
read_config ← {serde, toml, miette, thiserror, tracing, types}
read_style  ← {figment, serde, miette, thiserror, tracing}
read_references ← {serde, toml, miette, thiserror, tracing}
  ↑
font ← {read_config, types, allsorts, harfrust, read-fonts, font-types, rayon, miette, thiserror, tracing}
  ↑
parser ← {types, memchr, miette, phf, thiserror}
  ↑
layout ← {font, parser, types, icu, lazy-regex, font-types, read-fonts, miette}
  ↑
pdf_gen ← {font, layout, read_config, types, pdf-writer}
  ↑
seiran (main) ← {cli, font, layout, parser, pdf_gen, read_config, read_references, read_style, types, miette, tracing}

cli ← {clap}

subcommand (variation-axes, ttc-names, script-langs) ← {cli, font, read_config, types, read-fonts, font-types, miette}
```

### クレート一覧

| クレート            | パス                      | 責務                                                                           |
| ------------------- | ------------------------- | ------------------------------------------------------------------------------ |
| **types**           | `crates/types/`           | プロジェクト全体で使用される共通型定義（`FontType`, `FontKind`, `FontMap`）    |
| **cli**             | `crates/cli/`             | コマンドライン引数の解析（clap derive）                                        |
| **read_config**     | `crates/read_config/`     | TOML メイン設定ファイルの読み込みとバリデーション                              |
| **read_style**      | `crates/read_style/`      | TOML スタイル設定ファイルの読み込み（figment によるデフォルト値マージ）        |
| **read_references** | `crates/read_references/` | TOML 参照定義ファイルの読み込み（CSL ベース文献情報）                          |
| **font**            | `crates/font/`            | フォント処理（読込、解析、シェーピング、サブセット化、バリアブルフォント対応） |
| **parser**          | `crates/parser/`          | テキストファイルのパース（Lexer → Parser → Evaluator → Document IR）           |
| **layout**          | `crates/layout/`          | ローワリング（DocNode → LayoutNode）とレイアウトエンジン（LayoutNode → Item）  |
| **pdf_gen**         | `crates/pdf_gen/`         | PDF バイナリ生成エンジン                                                       |
| **seiran**          | `crates/seiran/`          | メインアプリケーション（エントリーポイント、パイプラインオーケストレーション） |
| **subcommand**      | `crates/subcommand/`      | サブコマンド実装（バリアブルフォント軸情報、TTC 名称、スクリプト/言語情報）    |

---

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: Rust の一般的な慣習とは異なり、関数の返り値では **`return` を必ず使用する**

   ```rust
   // ✅ Good
   pub fn calc(x: i32) -> i32 {
     let result = x * 2;
     return result;
   }

   // ❌ Bad（このプロジェクトでは非推奨）
   pub fn calc(x: i32) -> i32 {
     let result = x * 2;
     result
   }
   ```

2. **インデント**: 2 スペース（`rustfmt.toml` で設定済み）
3. **最大行幅**: 120 文字
4. **use 文**: `*` を避け明示的にインポート。`StdExternalCrate` でグループ化
5. **命名規則**: Rust 標準規約（`snake_case`, `CamelCase`）
6. **ドキュメントコメント**: すべてのモジュール、構造体、関数に **日本語** で記述
7. **Clippy**: `clippy::all` が deny、`pedantic` が warn。`needless_return` は allow

### エラーハンドリング

- **`thiserror`** + **`miette`** によるエラー型定義を必須とする
- `#[derive(thiserror::Error, Debug, miette::Diagnostic)]` を併用
- `#[diagnostic(code(...), help("..."))]` でユーザーフレンドリーな診断情報を付与
- `#[source]` 属性でエラーチェーンを形成
- `?` 演算子でエラーを伝播
- `Box<dyn std::error::Error>` は `main` のみに限定

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

### ドキュメントコメント形式

```rust
//! モジュールレベルのドキュメント

/// 関数やメソッドの説明
///
/// # Arguments
///
/// * `param` - パラメータの説明
///
/// # Returns
///
/// 戻り値の説明
///
/// # Errors
///
/// エラー条件の説明
///
/// # Panics
///
/// パニック条件の説明（もしあれば）
///
/// # Examples
///
/// 必要に応じて使用例をコードブロックで示す
pub fn example(param: Type) -> Result<ReturnType, Error> {
  // 実装
}
```

---

## ビルドとコマンド

| コマンド                                           | 説明                                      |
| -------------------------------------------------- | ----------------------------------------- |
| `cargo build`                                      | デバッグビルド                            |
| `cargo build --release`                            | リリースビルド（LTO 有効、strip symbols） |
| `cargo run build <text_file>`                      | テキストファイルから PDF を生成           |
| `cargo run variation-axes <font> [--font-index N]` | バリアブルフォント軸情報を表示            |
| `cargo run ttc-names <ttc_file>`                   | TTC ファイル内のフォント名一覧を表示      |
| `cargo run script-langs <font> [--font-index N]`   | サポートされるスクリプト / 言語を表示     |
| `cargo fmt`                                        | コードフォーマット                        |
| `cargo clippy`                                     | リント                                    |
| `cargo test`                                       | テスト実行                                |

---

## 設定ファイル

3 つの TOML 設定ファイルで PDF 生成をカスタマイズします。

### メイン設定（`config/config.toml`）

PDF 生成の基本設定を定義します。

#### 構造

```toml
name = "document_name"         # ドキュメント名（出力PDFファイル名）
references_path = "config/references.toml"  # 参照定義ファイルパス（オプション）
style_path = "config/style.toml"            # スタイル設定ファイルパス（オプション）

[pdf]
output_dir = "target/"         # 出力ディレクトリ
height = 842.0                 # ページ高さ（pt, A4 = 842）
width = 595.0                  # ページ幅（pt, A4 = 595）
font_size = 10.0               # デフォルトフォントサイズ（pt）
line_height_factor = 1.05      # 行間係数
margin_top = 99.0              # 上余白（pt）
margin_bottom = 99.0           # 下余白（pt）
margin_left = 85.0             # 左余白（pt）
margin_right = 85.0            # 右余白（pt）
# background_r = 0.8           # 背景色 R（0.0–1.0、オプション、RGB全指定必須）
# background_g = 0.7           # 背景色 G
# background_b = 0.6           # 背景色 B

[font_configs.<font_type>]     # 19種別それぞれに設定
font_name = "MyFont"           # PDF 内フォント名（一意必須）
font_path = "fonts/MyFont.ttf" # フォントファイルパス
script = "latn"                # OpenType スクリプトタグ（4文字）
language = "JAN"               # 言語タグ（3–4文字、オプション）
font_index = 0                 # TTC 内インデックス（オプション）
variation_axes = [             # バリアブル軸（オプション）
  { name = "wght", value = 400.0 }
]
features = [                   # OpenType フィーチャー（オプション）
  { tag = "kern", value = 1 }
]
```

### スタイル設定（`config/style.toml`）

見出しのフォントサイズと下余白を定義します。`figment` によるデフォルト値マージが行われるため、変更したい項目のみ記述すれば十分です。

```toml
font_size = 12.0               # 本文フォントサイズ（デフォルト: 12.0）

[part]
font_size = 40.0               # デフォルト: 40.0
bottom_margin = 20.0           # デフォルト: 20.0

[chapter]
font_size = 25.0               # デフォルト: 25.0
bottom_margin = 15.0           # デフォルト: 15.0

[section]
font_size = 20.0               # デフォルト: 20.0
bottom_margin = 10.0           # デフォルト: 10.0

[subsection]
font_size = 16.0               # デフォルト: 16.0
bottom_margin = 10.0           # デフォルト: 10.0

[paragraph]
font_size = 14.0               # デフォルト: 14.0
bottom_margin = 5.0            # デフォルト: 5.0

[subparagraph]
font_size = 12.0               # デフォルト: 12.0
bottom_margin = 5.0            # デフォルト: 5.0
```

### 参照定義（`config/references.toml`）

CSL ベースの文献情報を定義します。

```toml
style = "IEEE"                 # 引用スタイル

[[references]]
id = "example"                 # 参照 ID（文中から参照するキー）
title = "Example Book Title"
type = "book"                  # CSL 文献タイプ（article, book, chapter, thesis 等）
[[references.authors]]
family = "Yamamoto"            # 姓
given = "Taro"                 # 名（オプション）
```

### 19 フォント種別

`serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`,
`sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`,
`monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`,
`math`,
`japanese_serif`, `japanese_serif_bold`,
`japanese_sans_serif`, `japanese_sans_serif_bold`,
`japanese_monospace`, `japanese_monospace_bold`

---

## 主要な外部依存クレート

| クレート                             | 用途                                                            |
| ------------------------------------ | --------------------------------------------------------------- |
| **clap**                             | CLI 引数パース（derive マクロ）                                 |
| **pdf-writer**                       | PDF バイナリ構築                                                |
| **harfrust**                         | テキストシェーピング（HarfBuzz の Rust 実装）                   |
| **allsorts**                         | フォントサブセット化 + バリアブルフォントインスタンス化         |
| **read-fonts** / **font-types**      | OpenType フォントテーブル解析                                   |
| **rayon**                            | 19 フォントの並列読込・処理                                     |
| **memmap2**                          | テキストファイルのメモリマップ読込                              |
| **icu**                              | Unicode プロパティ（Script 判定、East Asian Width）             |
| **serde** + **toml**                 | TOML 設定ファイルデシリアライズ                                 |
| **figment**                          | スタイル設定ファイルのデフォルト値マージ読み込み                |
| **miette** + **thiserror**           | エラー診断（fancy 表示、diagnostic code、help）                 |
| **tracing** + **tracing-subscriber** | 構造化ロギング                                                  |
| **phf**                              | コンパイル時パーフェクトハッシュ（コマンド / 環境ディスパッチ） |

---

## テスト

- テスト実行: `cargo test`
- テスト用入力: `tests/text/text.txt`
- テスト用フォント: `tests/fonts/`
- 方針: 正常系・異常系・エッジケースをカバー。AAA パターン（Arrange / Act / Assert）で記述

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_function_name() {
    // Arrange
    let input = ...;

    // Act
    let result = function_under_test(input);

    // Assert
    assert_eq!(result, expected);
  }
}
```

---

## 開発フロー

1. **設計** — 変更の影響範囲を確認し、適切なクレートを選択
2. **実装** — コーディング規約に従って実装（特に `return` 必須ルール）
3. **テスト** — ユニットテストを追加し、`cargo test` で確認
4. **ドキュメント** — 日本語ドキュメントコメントを更新
5. **フォーマット** — `cargo fmt`
6. **リント** — `cargo clippy`
7. **ビルド確認** — `cargo build --release`

---

## パフォーマンスガイドライン

- **並列処理**: 重い処理には `rayon` を使用（例: 19 フォントの並列読込）
- **メモリ効率**: 大きなファイルには `memmap2` を使用。不要な `clone()` を避ける
- **フォントサブセット化**: 使用グリフのみを埋め込み、PDF サイズを最小化
- **効率的コレクション**: `IndexSet`, `HashMap` を活用
