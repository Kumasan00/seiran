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

| #   | 原則（要約）                                                                     | 目的   |
| --- | -------------------------------------------------------------------------------- | ------ |
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
cargo clippy --all-targets --all-features -- -D warnings   # リント（CI / pre-commit と同じ形）
cargo test                                                 # テスト実行
cargo test -p <crate_name>                                 # 特定クレートのテスト実行
```

`cargo fmt` は **nightly toolchain が必須**です。`rustfmt.toml` で `unstable_features = true`（`group_imports = "StdExternalCrate"` / `imports_granularity = "Crate"` / `format_macro_bodies` 等）を有効化しているためです。`build` サブコマンドの `-c` / `--config` を省略した場合は `./config/config.toml` が使用されます。

## アーキテクチャ

ここにはデータフローと依存の**骨格**だけを置く。**crate / module 別の実装構造・不変条件の正典は
`docs/architecture.md`** — 特定の crate / module を触る前に必ず該当節を読む。

### データフロー

```text
CLI 引数パース → TOML 設定読込（config.toml / style.toml / references）
  → frontend           字句・構文解析・評価: Lexer → Parser → CST → HIR（document::HirDocument）
  → semantics::analyze 意味解析: HIR 1 走査で SemanticFacts を確定し、引用があれば CSL 読込
                       → 整形・書誌生成 → SemanticDocument（HIR + 事実 + 生成物）
  → font               フォント読込・検証（FontResources / FontSystem）
  → typeset::layout    組版: SemanticDocument + 設定 + FontSystem → LaidOutDocument
                       内部順序（画像読込・寸法確定 → lowering → (a) block → (c+d) breaking
                       → 前付け・後付け → ページラベル → 走り文 → outline）は typeset に閉じる
  → seiran-pdf         (e) render: 確定座標の描画のみ（krilla がフォントサブセット化を内部実施）
  → seiran (CLI)       atomic write でファイル出力
```

段横断の要点（詳細・根拠は `docs/architecture.md`）:

- 外部資源（設定・スタイル・文献・CSL・ソース・フォント・画像）は例外なく `project::ProjectSource` 経由。
  compiler 側のコードは `std::fs` を直接呼ばない。資源を指すパスは `ProjectPath` 1 種類
- 採番・`\ref` 解決・引用キー検証は semantics が確定し、lowering は構造値を style の表示側フィールドで
  文字列にするだけ。文書木への書き戻しはどの段も行わない
- box は (a) で width / height / depth を 1 回だけ計測して保持し、以降のパスはフォントに触れない
- 行分割は Knuth–Plass（段落全体最適、既定は両端揃え。貪欲法 `GreedyBreaker` も併存）。分割可能点は
  ICU `LineSegmenter`（UAX #14）+ 欧文語中の discretionary ハイフネーション。縦組版（`break_pages`）も
  glue / penalty モデルで widow / orphan・keep-with-next・下端揃え（flush_bottom）を制御
- 数式は閉じた箱（`HBoxContent::Atom`）として行分割をまたがない
- 脚注はページ下部の脚注エリアぶん本文の実効下限を縮めて配置し、収まらなければ組版済みの行単位で
  次ページへ繰り越す。ページ単位採番（`[footnote]` の `numbering = "per_page"`）のときだけ本文パスを
  不動点まで反復する（`typeset::pagination::footnote_numbering` の専用 solver に閉じる。既定の通し採番は
  1 回で確定）
- `compile` は PDF バイト列の生成・保存を行わない。`seiran_pdf::render` と atomic write は CLI（`seiran`）の責務

### クレート構成

crate はデプロイ・外部依存・独立再利用の単位に限る（コンパイル段階を crate 境界にしない）。

```text
seiran-pdf       (e) 描画。workspace 内依存なし。境界型は自前の leaf 型のみで compiler 内部型を知らない
  ↑ seiran-compiler  言語処理・意味解決・組版のライブラリ（lib target のみ）。公開 API は compile 一本
      ↑ seiran       CLI（package 名・binary 名とも seiran）。compile → render → atomic write → 表示の 4 手順のみ
```

### `seiran-compiler` の module（すべて非公開、公開 API は `lib.rs` の `pub use` に一本化）

| module | 責務 1 行 | 依存先（crate 内） |
| --- | --- | --- |
| `length` / `color` | `Length`（sp = 1/65536pt の整数）/ `Color`（`#rrggbb`）の leaf 値型 | なし |
| `source` | ソースの同一性 `SourceId` と位置 `Span`（字句解析時点から存在する概念） | なし |
| `project` | 外部資源取得 seam（`ProjectPath` / `ProjectSource`、filesystem / memory の 2 実装） | なし |
| `document` | authored HIR（`HirDocument` / `NodeId` / `SourceMap` / `HirBuilder`）と HIR が値として持つ語彙型の所有者 | length color font source project |
| `config` | config.toml / style.toml の読込・garde 検証、意味解析への投影 `DocumentPolicy`、横断検証と `column_width` | length color document font project |
| `frontend` | 字句・構文解析（CST は非公開）→ HIR への評価変換。phf レジストリでディスパッチ、採番なし | document length color font source project |
| `semantics` | 意味解析 `analyze`（ラベル・`\ref`・カウンタ・見出し・引用キー検証）+ CSL 読込・引用表示 / 書誌生成 → `SemanticDocument`。引用まわりは子 module `citation` | document config font source project |
| `font` | フォント読込・シェーピング・検証。`FontKind` / `FontType` / `FontMap` / 処理済みフォント設定の所有 | length color project |
| `typeset` | 組版。入口は `layout` 1 操作（`SemanticDocument` → `LaidOutDocument`）。中間型は `boxes`、画像は `image`、段順序は `pagination` に閉じる | font config document semantics length color project seiran-pdf（画像デコードの leaf 関数のみ） |
| `compiler` | compile facade。全体の phase 順序と `Publication` への写像だけを持ち、組版中間型を名指ししない | 上記すべて + seiran-pdf |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない。Clippy の `needless_return` を allow にしているのはこの規約の裏返し）
2. **フォーマット**: 正典は `rustfmt.toml`（インデント 2 スペース・最大行幅 120 文字ほか）。手書き時もこれに合わせ、適用は `cargo +nightly fmt`（nightly 必須の理由は「コマンド」節を参照）
3. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`。型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい
4. **ドキュメントコメント**: すべてのモジュール・型（struct / enum / trait）・関数に **日本語** で記述
5. **`unreachable!` は積極的に使う**: まず型設計で到達不能な状態自体を表現不能にできないか検討し、それでも残る「絶対に到達しない」分岐は `_ => {}` / `Default::default()` / 黙って `Ok` を返す等でごまかさず `unreachable!` で落とす（不変条件の破れを最寄りで顕在化させる）。ただし入力（ソース・設定ファイル）由来で到達しうる状態は panic ではなく miette 診断エラーにする（`error-handling` skill 参照）。本体コードでは「なぜ到達しないか」＝上流のどの検証が保証しているかをメッセージに書く（例: `unreachable!("許可リスト外は strict_command_calls がエラーにする")`。テストの let-else 分解など自明な箇所は省略可）

### モジュール構成

- **`mod.rs` を使わない**: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

  例外: 統合テスト（`tests/`）の共通ヘルパは慣例どおり `tests/common/mod.rs` に置く（`common.rs` だとテストファイルとして扱われるため）。

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` / `pub(crate) mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `font::shaper` は `typeset::block` が `UnicodeBuffer` を直接参照するため `pub(crate) mod`、`config::test_support` は `#[doc(hidden)]` の再エクスポート。crate root 直下の非公開 module は crate 全体から到達できるため、garde カスタムバリデータを持つ `length` に `pub(crate)` は不要）。同名の型を 2 つ作って module 公開で回避しない — 名前側を変えて衝突自体を無くす（例: `ConfigValidationError` / `StyleValidationError`）。root facade へ載せるのは実際に名指しされる名前だけで、内部フィールド型としてしか現れない名前は再エクスポートしない。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### エラーハンドリング・バリデーション

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語、`code` は `<crate>::<category>::<name>` で階層化）。`miette::Result<T>` は `main` / 上位パイプライン関数のみで使う
- 設定値検証は `garde` の `#[derive(Validate)]` で宣言的に書き、違反は `MultipleValidationErrors` に集約して 1 度に報告する
- バリアント設計・`#[label]` / `NamedSource` によるソース位置付与・`#[related]` 集約の制約・garde パターンの詳細は `error-handling` skill を参照する。新しいエラー型の定義・バリアント追加・バリデーション追加の際は必ず参照すること

### Clippy

正典は root `Cargo.toml` の `[workspace.lints.clippy]`（各クレートは `lints.workspace = true` で継承）。

- `clippy::all` が deny、`pedantic` が warn。`needless_return` / `similar_names` / `too_many_lines` は allow
- restriction lint の `implicit_return` / `missing_docs_in_private_items` を warn で追加有効化している。「必須ルール」1（`return` 必須）はこれで機械的に強制され、4（doc コメント）は**有無だけ**が検査される（日本語で書かれているかは検査されないので人が見る）
- CI と pre-commit フックは `cargo clippy --all-targets --all-features -- -D warnings` で走る。warn レベルの指摘もそこでビルド失敗になるため、素の `cargo clippy` ではなくこの形で確認する
- `unwrap_used` / `expect_used` は**有効化していない**（restriction lint で `all` にも `pedantic` にも含まれない）。テストモジュールに付けている `#[allow(clippy::unwrap_used)]` は現状 lint を抑制しておらず、意図表明にとどまる

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei` ファイル群）、フォント: リポジトリ直下の `fonts/`
- AAA パターンで記述し、`// Arrange` / `// Act` / `// Assert` コメントで区切る
- テストコードでは `unwrap` / `expect` を許容する。テストモジュールには `#[allow(clippy::unwrap_used)]` を付け、`expect` のメッセージは日本語で期待を書く（例: `"一時ファイルを作成できるはず"`）
- **golden テスト・組版変更の検証**: レイアウトダンプ golden（`crates/seiran-compiler/src/compiler/golden.rs`）と PDF バイト比較の使い分け、前提資産の取得（初回は `tools/fetch-test-assets.sh` を 1 度実行）、golden の再生成、新機能へのテスト追加は `verify-typesetting` skill を参照する

## コード検索

rust-analyzer の LSP が設定済み。シンボルを辿る用途では grep ではなく `LSP` ツールを使う（deferred tool なので `ToolSearch("select:LSP")` でスキーマを読み込んでから呼ぶ）。

| 用途                                                                              | 操作                             |
| --------------------------------------------------------------------------------- | -------------------------------- |
| 定義へ移動（特に root facade の `pub use` re-export 越し。grep は facade で止まる） | `goToDefinition`                 |
| 参照の網羅（リファクタの影響範囲確認。grep の文字列一致は同名衝突・漏れが出る）    | `findReferences`                 |
| trait 実装の列挙（`LineBreaker` 等の seam の実装は複数クレートに散る）             | `goToImplementation`             |
| 型・シグネチャ・doc コメントの確認（宣言まで飛ばずに済む）                         | `hover`                          |
| ファイル内の型・関数一覧                                                          | `documentSymbol`                 |
| 呼び出し関係（パイプラインのどの段から呼ばれるか）                                | `incomingCalls` / `outgoingCalls` |

grep が正しいのは、文字列・パターン・命名規則の洗い出し、TODO や特定リテラルの検索、`.sei` / `.toml` などシンボルを持たないファイルの検索。

`LSP` は position 指定（`filePath` + `line` + `character`）が必須でシンボル名だけでは引けないため、**grep / Glob で位置を特定 → LSP で辿る** の順で使う。

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                                                                                           |
| ----------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ・余白、`[pdf].show_bookmarks`（しおり出力）、`[image]`（画像 DPI / downsample）、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語 |
| `style.toml`                        | **見た目**                 | 見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、脚注の体裁と採番方式、段組み数、参照リンク色、フロート挙動デフォルト                                         |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                                                                                 |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）
- **値の基本書式**: 長さ（`Length`）は単位付き文字列 `"12pt"` / `"5mm"`（素の数値は不可）、色（`Color`）は `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問、`[r, g, b]` 配列は不可）
- **style.toml の詳細スキーマ**（キャプションと番号 3 系統・見出し 2 レイヤーマージ・カウンタ固定 9 種・`[math.script]` / `[math.block]`・`[page]` の `flush_bottom` 等）は `docs/architecture.md` の config（style）節を参照

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`

## issue / PR 運用

issue・PR・branch・commit・ラベル・sub-issue の運用規約は `issue-pr-ops` skill に集約。
GitHub 上で issue / PR を作る・編集する、branch を切る、commit メッセージや merge 方法を決める、
ラベルや epic / sub-issue の親子関係を判断する際は、その skill を参照すること。
クレート構成・パイプライン・設定スキーマ・CLI に触れる PR を仕上げる際は
`docs-sync` skill のチェックリストでドキュメント更新漏れを確認すること。
