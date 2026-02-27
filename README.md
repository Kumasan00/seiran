# seiran

![青藍](images/seiran.jpg "青藍")

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースのコマンドラインツールです。

## 特徴

- TeX ライクなコマンド構文（`\part`, `\chapter`, `\section` など）によるドキュメント構造化
- 19 種類のフォント種別に対応（Serif / Sans-serif / Monospace × Regular / Bold / Italic / Bold Italic + Math + 日本語フォント）
- バリアブルフォント対応（軸の指定によるウェイト・幅の調整）
- OpenType フィーチャー（カーニング、リガチャなど）のサポート
- フォントサブセット化による PDF サイズの最小化
- TOML 設定ファイルによる柔軟なカスタマイズ
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
cargo run build <text_file_path>
```

テキストファイルと `config/config.toml` の設定に基づいて PDF を生成します。

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

`config/config.toml` で PDF 生成の設定を行います。

```toml
name = "document_name"         # ドキュメント名（出力PDFファイル名）

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

## プロジェクト構成

```text
crates/
├── cli/              # コマンドライン引数の解析
├── font/             # フォント処理（読込・シェーピング・サブセット化）
├── parser/           # テキストパース（Lexer → Parser → Evaluator → Layout Engine）
├── pdf_gen/          # PDF 生成エンジン
├── read_config/      # TOML 設定ファイルの読み込みと検証
├── read_style/       # TOML スタイルファイルの読み込みと検証
├── seiran/           # メインアプリケーション（エントリーポイント）
└── types/            # 共通型定義
```

## License

Copyright 2026 Kuma
Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](http://opensource.org/licenses/MIT)

at your option.
