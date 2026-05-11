# seiran

![青藍](images/seiran.jpg "青藍")

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースのコマンドラインツールです。

## 特徴

- TeX ライクなコマンド構文（`\part`, `\chapter`, `\section` など）によるドキュメント構造化
- 19 種類のフォント種別に対応（Serif / Sans-serif / Monospace × Regular / Bold / Italic / Bold Italic + Math + 日本語フォント）
- バリアブルフォント対応（軸の指定によるウェイト・幅の調整）
- OpenType フィーチャー（カーニング、リガチャなど）のサポート
- フォントサブセット化による PDF サイズの最小化
- TOML 設定ファイルによる柔軟なカスタマイズ（メイン設定 / スタイル設定 / 参照定義）
- スタイル設定ファイルによる見出しフォントサイズ・余白のカスタマイズ
- CSL ベースの参照定義ファイルによる文献管理
- 詳細なエラー診断メッセージ（miette による fancy 表示）

## 必要環境

- Rust (Edition 2024)
- macOS（M4 MacBook Pro / Tahoe 26.3 で動作確認済み）
- Xcode Command Line Tools (Xcode 26)

## インストール

```sh
git clone https://github.com/Kumasan00/seiran.git
cd seiran
cargo build --release
```

## 使い方

### PDF の生成

```sh
cargo run -- build [-c <config_path>]
```

設定ファイル（既定 `./config/config.toml`）の `sources` 配列に列挙されたテキストファイルを順次パース・結合して PDF を生成します。`sources = ["chapter1.txt", "chapter2.txt"]` のように複数ファイルを指定できます。

### バリアブルフォント軸情報の表示

```sh
cargo run variation-axes <font_path> [--font-index <index>]
```

### TTC ファイル内のフォント名一覧

```sh
cargo run ttc-names <ttc_file_path>
```

### フォントのスクリプト / 言語情報

```sh
cargo run script-langs <font_path> [--font-index <index>]
```

## 設定

### メイン設定（`config/config.toml`）

PDF 生成の基本設定を行います。

```toml
name = "document_name"         # ドキュメント名（出力PDFファイル名）
references_path = "config/references.toml"  # 参照定義ファイルパス（オプション）
style_path = "config/style.toml"            # スタイル設定ファイルパス（オプション）

[pdf]
output_dir = "target/"         # 出力ディレクトリ
height = 842.0                 # ページ高さ（pt, A4 = 842）
width = 595.0                  # ページ幅（pt, A4 = 595）
margin_top = 99.0              # 上余白（pt）
margin_bottom = 99.0           # 下余白（pt）
margin_left = 85.0             # 左余白（pt）
margin_right = 85.0            # 右余白（pt）

[font_configs.serif]           # 19種別それぞれに設定
font_name = "MyFont"           # PDF 内フォント名（一意必須）
font_path = "fonts/MyFont.ttf" # フォントファイルパス
script = "latn"                # OpenType スクリプトタグ
language = "JAN"               # 言語タグ（オプション）
font_index = 0                 # TTC 内インデックス（オプション）
variation_axes = [             # バリアブル軸（オプション）
  { name = "wght", value = 400.0 }
]
features = [                   # OpenType フィーチャー（オプション）
  { tag = "kern", value = 1 }
]
```

### スタイル設定（`config/style.toml`）

見出しのフォントサイズと下余白をカスタマイズします。指定しない項目はデフォルト値が使用されます。

```toml
font_size = 12.0               # 本文フォントサイズ（pt）
line_height_factor = 1.05      # 行間係数（> 0）
# background_color = [0.8, 0.7, 0.6]  # 背景色 RGB（各 0.0–1.0、オプション）

[part]
font_size = 40.0               # Part 見出しフォントサイズ
bottom_margin = 20.0           # Part 見出し下余白

[chapter]
font_size = 25.0
bottom_margin = 15.0

[section]
font_size = 20.0
bottom_margin = 10.0

[subsection]
font_size = 16.0
bottom_margin = 10.0

[paragraph]
font_size = 14.0
bottom_margin = 5.0

[subparagraph]
font_size = 12.0
bottom_margin = 5.0
```

### 参照定義（`config/references.toml`）

CSL ベースの文献情報を定義します。

```toml
style = "IEEE"                 # 引用スタイル

[[references]]
id = "example"                 # 参照 ID
title = "Example Book Title"
type = "book"                  # CSL 文献タイプ（book, article 等）
[[references.authors]]
family = "Yamamoto"
given = "Taro"
```

## プロジェクト構成

```text
crates/
├── cli/              # コマンドライン引数の解析
├── font/             # フォント処理（読込・シェーピング・サブセット化）
├── layout/           # レイアウトエンジン（Document IR → LayoutNode → Item）
├── parser/           # テキストパース（Lexer → Parser → Evaluator → Document IR）
├── pdf_gen/          # PDF 生成エンジン
├── read_config/      # TOML メイン設定ファイルの読み込みと検証
├── read_references/  # TOML 参照定義ファイルの読み込み
├── read_style/       # TOML スタイル設定ファイルの読み込み
├── seiran/           # メインアプリケーション（エントリーポイント）
├── subcommand/       # サブコマンド（バリアブルフォント軸情報、TTC 名称、スクリプト/言語情報）
└── types/            # 共通型定義
```

## License

Copyright 2026 Kuma
Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](http://opensource.org/licenses/MIT)

at your option.
