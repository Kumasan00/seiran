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

設定ファイル（既定 `./config/config.toml`）の `sources` 配列に列挙されたテキストファイルを順次パース・結合して PDF を生成します。`sources = ["chapter1.sei", "chapter2.sei"]` のように複数ファイルを指定できます。

### バリアブルフォント軸情報の表示

```sh
cargo run -- variation-axes <font_path> [--font-index <index>]
```

### TTC ファイル内のフォント名一覧

```sh
cargo run -- ttc-names <ttc_file_path>
```

### フォントのスクリプト / 言語情報

```sh
cargo run -- script-langs <font_path> [--font-index <index>]
```

## 設定

### メイン設定（`config/config.toml`）

PDF 生成の基本設定を行います。長さの値はすべて単位付き文字列（`"12pt"` / `"5mm"`）で指定します。

```toml
sources = ["chapter1.sei", "chapter2.sei"]  # 入力テキストファイル
style_path = "config/style.toml"            # スタイル設定ファイルパス（オプション）
references_path = "config/references.toml"  # 参照定義ファイルパス（オプション）

[document]                     # PDF メタデータ（すべてオプション）
title = "ドキュメントタイトル"
author = "著者名"
date = "2026-01-01"
language = "ja"                # 文書全体の言語（BCP 47。ハイフネーション等が参照）

[output]
name = "document_name"         # 出力 PDF ファイル名
output_dir = "target/"         # 出力ディレクトリ

[pdf]
height = "842pt"               # ページ高さ（A4 = 842pt）
width = "595pt"                # ページ幅（A4 = 595pt）
margin_top = "99pt"            # 上余白
margin_bottom = "99pt"         # 下余白
margin_left = "85pt"           # 左余白
margin_right = "85pt"          # 右余白
# show_bookmarks = true        # しおり出力（既定 true）

# [image]                      # ラスタ画像の埋め込み解像度（既定 max_dpi=300 / downsample=true）
# max_dpi = 300
# downsample = true

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

本文・見出し・図表・数式などの見た目をカスタマイズします。部分指定した項目だけがデフォルト値に上書きマージされます。

```toml
# background_color = "#ccb599"  # 背景色（"#rrggbb" 16 進文字列、オプション）

[text]
font_size = "12pt"             # 本文フォントサイズ
line_height_factor = 1.05      # 行間係数（> 0）

[heading.part]                 # 見出しレベルは part / chapter / section /
format = "第{number}部 {title}" # subsection / paragraph / subparagraph の 6 種
font_size = "40pt"             # 見出しフォントサイズ
bottom_margin = "20pt"         # 見出し下余白

[heading.chapter]
format = "第{number}章 {title}"
font_size = "25pt"
bottom_margin = "15pt"

[heading.section]
format = "{number} {title}"
font_size = "20pt"
bottom_margin = "10pt"
```

### 参照定義（`config/references.toml`）

CSL ベースの文献情報を定義します。トップレベルのテーブルキーがそのまま参照 ID になります
（`references.` 接頭辞は不要）。引用スタイル（`.csl`）の選択は見た目設定として `style.toml` の
`[reference].csl_path` に置きます。

```toml
[example]                      # テーブルキー = 参照 ID
title = "Example Book Title"
type = "book"                  # CSL 文献タイプ（book, article 等）
[[example.author]]
family = "Yamamoto"
given = "Taro"
```

## プロジェクト構成

```text
crates/
├── citation/         # 参照定義ファイルの読込・\cite の CSL 整形（引用の採番・書誌生成、references 子 module）
├── config/           # TOML メイン設定・スタイル設定ファイルの読み込みと検証（read_config / read_style 子 module）
├── font/             # フォント処理（読込・シェーピング・検証）
├── frontend/         # 字句解析・構文解析・評価（Lexer → Parser → CST → Document IR。CST は非公開）
├── model/            # 全段共有のデータモデル（共通型・Document IR・組版コア型）
├── pdf_gen/          # PDF 生成エンジン（確定座標の描画）
├── seiran/           # メインアプリケーション（エントリーポイント。CLI 引数解析・サブコマンドを内包）
└── typeset/          # Document IR → LayoutNode 変換・シェーピング・計測・行分割・縦組版（lowering / layout / hlist 子 module）
```

## License

Copyright 2026 Kuma
Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](http://opensource.org/licenses/MIT)

at your option.
