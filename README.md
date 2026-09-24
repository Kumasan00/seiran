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
- CSL ベースの参照定義ファイルによる文献管理
- 詳細なエラー診断メッセージ

## 必要環境

- Rust（Edition 2024 対応ツールチェーン）
- macOS / Linux（CI は Linux で実行）

## インストール

```sh
git clone https://github.com/Kumasan00/seiran.git
cd seiran
cargo build --release
```

## 使い方

### PDF の生成

```sh
cargo run -- build [-c <config_path>] [-v|-vv|-vvv] [-q] [--log-file <path>]
```

設定ファイル（既定 `./config/config.toml`）の `sources` 配列に列挙されたテキストファイルを順次パース・結合して PDF を生成します。`sources = ["chapter1.sei", "chapter2.sei"]` のように複数ファイルを指定できます。

`-v` / `-q` / `--log-file` は下記のフォント調査サブコマンドにも共通です。

ログは標準エラー出力へ出ます。`-v` / `-vv` / `-vvv` で詳しくなり（工程 / 内部詳細 / 最大）、`-q` は端末への警告・ログ・サマリを止めます。`-v` では工程（compile とその子の input / frontend / semantics / font / typeset、render、write）ごとに「工程を開始」と「工程を終了」（`status=Succeeded` または `Failed` と所要時間 `elapsed`）が出るので、失敗・中断した実行でもどの工程で止まったかが分かります。各行には所属する工程が `compile:typeset:` のような prefix で付きます。target 単位で絞りたいときは環境変数 `RUST_LOG`（例: `RUST_LOG=seiran_compiler::typeset=trace`）を使います。`RUST_LOG` が有効なときは `-v` より優先され、両方を指定すると `-v` を無視した旨の警告が出ます（`RUST_LOG` を解釈できないときも警告し、`-v` の設定で続けます）。この警告は `RUST_LOG` の値に関わらず、`-q` でなければ端末に、`--log-file` 指定時はファイルに必ず出ます。

`--log-file <path>` を付けると、端末の表示はそのままに同じログをファイルへも残します。ファイルには時刻が付き、ANSI 装飾は入りません。実行ごとに新規作成され（既存のパスを指定するとエラーで止まり、そのファイルには触れません）、親ディレクトリが無ければ作られます。PDF の保存先と同じパスは保存前に拒否します。`-v` の工程行（「工程を開始」「工程を終了」等の INFO 以上）は書くたびにファイルへ flush されるので、ハングしたり `Ctrl-C` 等で中断したりした実行でも、そこまでの工程の開始・終了をファイルから読めます（`-vv` / `-vvv` が増やす内部詳細行はまとめて書かれ、実行が終わるかログの書き込みが確定した時点でファイルへ反映されます）。ログの書き込みに失敗した実行は、本処理が成功していても終了コード 1 で終わります（生成済みの PDF は残ります）。`-q` と `-v` は独立で、`-q -vv --log-file run.log` は端末を黙らせたままファイルへ内部詳細まで記録するので、静かに回して後から読み返せます。ビルドが失敗したときは、その診断も（`-q` でも）終了記録の直前に残ります。

ファイルに入るのは次のとおりです。

- 先頭の実行記録: 開始時刻・バージョン・サブコマンド・基準ディレクトリ・実効フィルタ（`-v` や `RUST_LOG` が無くても必ず書かれます）
- ログ: `-v` / `RUST_LOG` に従う tracing の行（`-v` で工程の開始・終了と件数、`-vv` で内部詳細、`-vvv` で行分割・シェーピングの候補と本文の抜粋）。各行に時刻が付きます
- warning 診断（`RUST_LOG` の警告を含む）・成功サマリ・ビルドを止めた診断（`-q` でも書かれます）
- 末尾の終了記録: 終了時刻と終了状態（成功 / 失敗）

### フォントの調査

```sh
cargo run -- variation-axes <font> [-f <font_index>]   # バリアブルフォントの軸とインスタンス
cargo run -- ttc-names <ttc_file>                      # TTC に含まれるフォント名
cargo run -- script-langs <font> [-f <font_index>]     # 対応するスクリプト / 言語
```

一覧は標準出力へ出ます。フォントの読み込み・解析に失敗すると一覧を出さずに終了コード 1 で止まります。

## 設定

### メイン設定（`config/config.toml`）

PDF 生成の基本設定を行います。長さの値はすべて単位付き文字列（`"12pt"` / `"5mm"` / `"1.5cm"`）で指定します
（単位は小文字の `pt` / `mm` / `cm`、数値と単位の間に空白を入れない）。ソースの `\image[width=5cm]` や
`\space{3pt}` も同じ書式です。
用紙そのものの寸法はここ（物理設定）、用紙のどこを本文領域にするか＝ページ余白は見た目なので
`style.toml` の `[page]` が持ちます。

```toml
sources = ["chapter1.sei", "chapter2.sei"]  # 入力テキストファイル
style_path = "config/style.toml"            # スタイル設定ファイルパス（オプション）
references_path = "config/references.toml"  # 参照定義ファイルパス（オプション）

[document]                     # 文書のメタデータ（すべてオプション。date 以外は PDF メタデータにも入る）
title = "ドキュメントタイトル"
author = "著者名"
date = "2026-01-01"            # 表紙・走り文に表示する日付（PDF メタデータには入らない）
language = "ja"                # 文書全体の言語（BCP 47。ハイフネーションと PDF メタデータ）
# subject = "主題"             # PDF メタデータの /Subject
# keywords = ["a", "b"]        # PDF メタデータの /Keywords

[output]
name = "document_name"         # 出力 PDF ファイル名
output_dir = "target/"         # 出力ディレクトリ（オプション。省略時は build を実行したカレントディレクトリ）

[pdf]
height = "842pt"               # ページ高さ（A4 = 842pt）
width = "595pt"                # ページ幅（A4 = 595pt）
# show_bookmarks = true        # しおり出力（既定 true）

# [image]                      # ラスタ画像の埋め込み解像度（既定 max_dpi=300 / downsample=true）
# max_dpi = 300
# downsample = true

[font_configs.serif]           # 19種別それぞれに設定
font_path = "fonts/MyFont.ttf" # フォントファイルパス
script = "latn"                # OpenType スクリプトタグ（オプション）
language = "en"                # BCP 47 言語タグ（オプション）
# ot_language = "ENG"          # OpenType 言語システムタグ（オプション。指定時は script 必須）
font_index = 0                 # TTC 内インデックス（オプション）
variation_axes = [             # バリアブル軸（オプション）
  { name = "wght", value = 400.0 }
]
features = [                   # OpenType フィーチャー（オプション）
  { tag = "kern", value = 1 }
]
```

### スタイル設定（`config/style.toml`）

本文・見出し・図表・数式などの見た目をカスタマイズします。部分指定した項目だけがデフォルト値に上書きマージされます。キーの一覧と既定値は `crates/seiran-compiler/src/style.rs` と `style/` 配下の各構造体の doc コメントを参照してください。

```toml
# background_color = "#ccb599"  # 背景色（"#rrggbb" 16 進文字列、オプション）

[page]                         # 用紙上の本文領域（余白）
margin_top = "99pt"            # 上余白（既定 99pt）
margin_bottom = "99pt"         # 下余白（既定 99pt）
margin_left = "85pt"           # 左余白（既定 85pt）
margin_right = "85pt"          # 右余白（既定 85pt）
# flush_bottom = false         # 下端揃え（既定 false）

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

### 参照定義（`config/references.toml` または `.json`）

CSL ベースの文献情報を定義します。TOML ではトップレベルのテーブルキーがそのまま参照 ID になります
（`references.` 接頭辞は不要）。拡張子が `.json` のファイルは CSL-JSON として読みます。
引用スタイル（`.csl`）の選択は見た目設定として `style.toml` の
`[reference].csl_path` に置きます。

```toml
[example]                      # テーブルキー = 参照 ID
title = "Example Book Title"
type = "book"                  # CSL 文献タイプ（book, article 等）
[[example.author]]
family = "Yamamoto"
given = "Taro"
```

## License

Copyright 2026 Kuma
Licensed under either of

- [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
- [MIT License](http://opensource.org/licenses/MIT)

at your option.
