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

設計合意済みの言語機能を実装へ落とす順序（設計ゲート → frontend レジストリ → … → golden → ドキュメント）は
`add-language-feature` skill を参照する。

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

`cargo fmt` は **nightly toolchain が必須**です。`rustfmt.toml` で `unstable_features = true`（`group_imports = "StdExternalCrate"` / `imports_granularity = "Crate"` / `format_macro_bodies` 等）を有効化しているためです。`build` サブコマンドの `-c` / `--config-path` を省略した場合は `./config/config.toml` が使用されます。

## アーキテクチャ

ここにはデータフローと依存の**骨格**だけを置く。**crate / module 別の実装構造・不変条件の正典は
`docs/architecture.md`** — 特定の crate / module を触る前に必ず該当節を読む。

### データフロー

```text
CLI 引数パース → compiler::input::load 入力読込: config.toml → style.toml → 横断検証
                       → references → フォントバイト列（project::FontData）→ sources
                       （順序とエラー集約は input に閉じる）
  → frontend           字句・構文解析・評価: Lexer → Parser → CST → HIR（document::HirDocument）
  → semantics::analyze 意味解析: HIR 1 走査で SemanticFacts を確定し、引用があれば CSL 読込
                       → 整形・書誌生成 → SemanticDocument（HIR + 事実 + 生成物）
  → typeset::layout    組版: SemanticDocument + 設定 + FontResources → LaidOutDocument
                       フォント資源の構築（typeset::font: 解析 → メトリクス → 検証 →
                       シェーパー）と内部順序（画像読込・寸法確定 → lowering → (a) block
                       → (c+d) breaking → 前付け・後付け → ページラベル → 走り文 → outline）
                       は typeset に閉じる
  → seiran-pdf         (e) render: compiler が確定させた Publication（純データ）を描画するのみ
                       （krilla フォントの構築・画像デコード・フォントサブセット化はここに閉じる）
  → seiran (CLI)       atomic write でファイル出力
```

段横断の要点（骨格のみ。詳細・根拠は `docs/architecture.md` の該当節）:

- 外部資源は例外なく `project::ProjectSource` 経由（compiler 側は `std::fs` を直接呼ばない）。
  資源を指すパスは `ProjectPath` 1 種類
- 入力読込の外向き入口は `compiler::input::load` 1 つ。読込順序とエラー集約を知るのは `input` だけで、
  成果物 `CompilationInputs` は検証を通った値しか持たない
- 採番・`\ref` 解決・引用キー検証は semantics が確定し、lowering は表示文字列化だけ。
  文書木への書き戻しはどの段も行わない
- box は (a) で寸法を 1 回だけ計測して保持し、以降のパスはフォントに触れない
- 行分割・縦組版とも glue / penalty モデル（行分割は Knuth–Plass が既定）
- 数式は閉じた箱（`HBoxContent::Atom`）として行分割をまたがない
- 脚注は本文の実効下限を縮めて配置し、行単位でページ間繰越。ページ単位採番のときだけ本文パスを
  不動点まで反復する（`typeset::pagination::footnote_numbering`）
- `compile` は PDF バイト列の生成・保存を行わない。`seiran_pdf::render` と atomic write は CLI（`seiran`）の責務

### クレート構成

crate はデプロイ・外部依存・独立再利用の単位に限る（コンパイル段階を crate 境界にしない）。
描画バックエンドが 1 つである間は `Renderer` trait も共有型だけの第三 crate も作らない（#372）。

```text
seiran-compiler    言語処理・意味解決・組版のライブラリ（lib target のみ）。組版成果物
                   （`Publication` 系 leaf 型）の型所有者。公開 API は compile + 成果 Compilation
                   （Publication / DependencyManifest / Warnings / BuildStatistics / OutputPlan）
                   + 失敗型 CompileFailure + 入力 seam（ProjectSource / ProjectPath）
  ↑ seiran-pdf     (e) 描画。compiler facade の Publication を消費して PDF バイト列を作る backend
                   （krilla / krilla-svg / 画像デコードはここに閉じる）
  ↑ seiran         CLI（package 名・binary 名とも seiran）。compile → render → atomic write → 表示の 4 手順のみ
```

`seiran-pdf` が読めるのは compiler の facade に載る `Publication` 系 leaf 型だけで、`ProjectConfig` /
`Style` / `typeset::Page` は facade に出ていない — 「renderer は確定座標の描画のみ・レイアウト判断ゼロ」
という防火壁は、この公開範囲の狭さが担っている。

### `seiran-compiler` の module（すべて非公開、公開 API は `lib.rs` の `pub use` に一本化）

並び順は `docs/architecture.md` の節順と同一（leaf 値型 → 入力 → 文書と設定 → パイプライン段 →
成果物 → facade）。依存関係・子 module 構成・不変条件は `docs/architecture.md` の各節が正典。

| module | 責務 1 行 |
| --- | --- |
| `length` / `color` | `Length`（sp = 1/65536pt の整数）/ `Color`（`#rrggbb`）の leaf 値型 |
| `failures` | 1 回の検査で見つけた複数の失敗を運ぶ非空集合 `Failures<E>`（空で構築不能・`Diagnostic` 非実装） |
| `source` | ソースの同一性 `SourceId` と位置 `Span` |
| `project` | プロジェクトの物理的な入力 — 外部資源取得 seam（`ProjectPath` / `ProjectSource`）+ config.toml の読込・検証 + `SourceSet` + フォント資源（子 module `font`） |
| `document` | authored HIR（`HirDocument` / `HirBuilder` / `SourceMap`）と HIR が値として持つ語彙型の所有者 |
| `style` | style.toml（見た目）のデータモデル・既定値・読込・検証。CSL 本体は読まない |
| `frontend` | 字句・構文解析（CST は非公開）→ HIR への評価変換。phf レジストリでディスパッチ、採番なし |
| `semantics` | 意味解析 `analyze`（採番・`\ref`・引用キー検証）+ CSL 読込・書誌生成 → `SemanticDocument` |
| `typeset` | 組版。入口は `layout` 1 操作（`SemanticDocument` → `LaidOutDocument`） |
| `publication` | 組版成果物の確定表現（`Publication` / `PaintOp` / 描画資源）。krilla を知らない純データ |
| `compiler` | compile facade。全体の phase 順序と `Publication` への写像だけを持ち、組版中間型を名指ししない |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない。Clippy の `needless_return` を allow にしているのはこの規約の裏返し）
2. **フォーマット**: 正典は `rustfmt.toml`（インデント 2 スペース・最大行幅 120 文字ほか）。手書き時もこれに合わせ、適用は `cargo +nightly fmt`（nightly 必須の理由は「コマンド」節を参照）
3. **use 文**: import は「名前を持ち込む」行為であり、**持ち込んだ名前の出自が呼び出し箇所の字面で一意に分かる範囲で最短パスを使う**（G1「字面だけで構造が一意」の import への適用）。以下はすべてこの原則から導かれる
   - **起点は `crate::` に統一**: `use` は `crate::` 起点で書き、`super::` / `self::` 起点は使わない（同じ型に 2 本のパスを作らないため。root ファサードと同じ理由）。例外は 2 つだけ — (a) 同じファイルが `mod` 宣言している子 module から取り込む相対 use（`use input::CompilationInputs;`。`mod input;` が同じ画面にあるので出自が字面で見え、隣の `pub use input::OutputPlan;` と同じ形になる）、(b) `#[cfg(test)]` の module（`mod tests` / `mod test_support`）が**直近の親**の被テスト項目を取り込む `use super::*` / `use super::Item`。(b) は親 1 段までで、`super::super::` で祖父母以上へ遡る形は使わない（何段上かを数えないと出自が分からず、原則に反する）
   - **`crate::` を本体コードへ直書きしない**: 型・トレイトは import して裸の名前で書き、関数は import した module 経由で `module::fn(...)` と呼ぶ。`clippy::absolute_paths` は 4 セグメント以上しか検出しない（末尾の enum variant / 関連関数を数えないので `crate::source::SourceId::new` は素通りする）ので、規約のほうが lint より厳しい。doc コメント内の intra-doc link（``[`crate::Foo`]``）は絶対パスが正しいので対象外
   - `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`
   - 型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい
   - `#[cfg(test)]` の項目だけが使う import は `#[cfg(test)] use ...;` と書いて本体ビルドから外す（本体の use ツリーへ混ぜると非テストビルドで unused になる）
4. **ドキュメントコメント**: すべてのモジュール・型（struct / enum / trait）・関数に **日本語** で記述
5. **`unreachable!` は積極的に使う**: まず型設計で到達不能な状態自体を表現不能にできないか検討し、それでも残る「絶対に到達しない」分岐は `_ => {}` / `Default::default()` / 黙って `Ok` を返す等でごまかさず `unreachable!` で落とす（不変条件の破れを最寄りで顕在化させる）。ただし入力（ソース・設定ファイル）由来で到達しうる状態は panic ではなく miette 診断エラーにする（`error-handling` skill 参照）。本体コードでは「なぜ到達しないか」＝上流のどの検証が保証しているかをメッセージに書く（例: `unreachable!("許可リスト外は strict_command_calls がエラーにする")`。テストの let-else 分解など自明な箇所は省略可）

### モジュール構成

- **`mod.rs` を使わない**: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

  例外: 統合テスト（`tests/`）の共通ヘルパは慣例どおり `tests/common/mod.rs` に置く（`common.rs` だとテストファイルとして扱われるため）。

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` / `pub(crate) mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `project::config` は入口を `project::config::load` と読ませて `style::load` と区別する。crate root 直下の非公開 module は crate 全体から到達できるため、garde カスタムバリデータを持つ `length` に `pub(crate)` は不要）。同名の型を 2 つ作って module 公開で回避しない — 名前側を変えて衝突自体を無くす（例: `ConfigValidationError` / `StyleValidationError`）。root facade へ載せるのは実際に名指しされる名前だけで、内部フィールド型としてしか現れない名前は再エクスポートしない。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### エラーハンドリング・バリデーション

正典は `error-handling` skill — 新しいエラー型の定義・バリアント追加・診断（code / help / label /
related）の設計・ソース位置付与・garde バリデーション追加の際は必ず参照する。常時効く原則は以下。

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語）。`miette::Result<T>` は CLI 入口だけで使い、内部パイプラインは具体的なエラー型を保つ
- `compile` の失敗型は不透明型 `CompileFailure`（先頭が主診断・空で構築不能）。ユーザーが最初に読むメッセージは常に修正可能な leaf diagnostic にする
- 診断 `code` の第 1 階層は「段」の固定列挙（`project` / `style` / `frontend` / `semantics` / `typeset` / `compiler` / `pdf` / `cli` の 8 つ）、第 2 階層以降は著者が選ぶ意味的カテゴリ（module パスではない）
- 設定値検証は `garde` で宣言的に書き、違反は `Failures<E>` に集めて 1 度に報告する
- 集約するかは「失敗後も独立な検査を安全かつ決定的に続けられるか」で決め（#376）、表示順は入力の論理順（`HashMap` の反復順や rayon の完了順に依存させない）
- 集約自身に診断 `code` を付けない（`Failures<E>` は `Diagnostic` を実装しないことで型保証）
- 外部資源の read error は低水準 cause として、所有段が作る leaf diagnostic の `#[source]` に入れる（#377）
- warning は error と公開型を共用せず（`Warnings`）、同じ問題を診断と tracing の両方で出さない（#377）

### Clippy

正典は root `Cargo.toml` の `[workspace.lints.rust]` / `[workspace.lints.clippy]`（各クレートは `lints.workspace = true` で両テーブルを継承）と、lint 側の設定値を持つ root `clippy.toml`。

- `clippy::all` が deny、`pedantic` が warn。`needless_return` / `similar_names` / `too_many_lines` / `result_large_err` は allow
- restriction lint の `absolute_paths` / `allow_attributes` / `allow_attributes_without_reason` / `clone_on_ref_ptr` / `implicit_return` / `iter_over_hash_type` / `map_err_ignore` / `missing_assert_message` / `missing_docs_in_private_items` / `mod_module_files` / `multiple_unsafe_ops_per_block` / `panic_in_result_fn` / `undocumented_unsafe_blocks` / `unnecessary_safety_comment` / `unused_result_ok` / `unwrap_used` を warn で追加有効化している。「必須ルール」1（`return` 必須）はこれで機械的に強制され、4（doc コメント）は rustc の `missing_docs`（公開項目）と 2 枚で全項目の**有無だけ**が検査される（日本語で書かれているかは検査されないので人が見る）
- 上記とは別枠で、**nursery lint** の `redundant_clone` / `derive_partial_eq_without_eq` を warn で有効化している（restriction ではないので同じ列には並べない）
- clippy.toml の設定値は `absolute-paths-max-segments = 3`（下の bullet）と `allow-unwrap-in-tests = true`（テスト内の `.unwrap()` を対象外にする）の 2 つ。
- `absolute_paths` は `clippy.toml` の `absolute-paths-max-segments = 3` で運用する。`project::config::load` と `style::load` を書き分ける既存のイディオム（3 セグメント）は温存され、`crate::` を頭に付けた 4 セグメント以上だけが落ちる。これは「use 文」規約の**最終防衛線**であって規約そのものではない — 規約は本体コードに `crate::` を直書きしないことを求めており、lint はそのうち検出できる分だけを機械化している。`#[cfg(test)]` の中でも発火するので、テストにも同じ規約が効く
- rustc 側は `elided_lifetimes_in_paths` / `missing_docs` / `unnameable_types` / `unreachable_pub` / `unsafe_op_in_unsafe_fn` / `unused_qualifications` を warn で明示有効化している（ほかに 0 件予防の 4 件は上の bullet）
  - `unreachable_pub` は「モジュールは既定で非公開 + root ファサード」を機械的に強制するもので、外から到達しない `pub` は `pub(crate)` / `pub(super)` に狭める（crate 外へ本当に公開する項目だけが `pub` として残る）。`unnameable_types` はその裏面で、公開シグネチャに現れるのに facade から名指しできない型を落とす — 直し方は 2 択で、公開 API の一部なら facade へ再エクスポートし、内部型なら宣言側の可視性を狭める
  - `unused_qualifications` は use 規約「最短パス」の機械化。スコープに入っている名前を `std::sync::Arc::new` のように再修飾しない（`absolute_paths` は 4 セグメント以上しか見ないので、こちらが短いパスの側を押さえる）
  - `missing_docs` は必須ルール 4 の公開項目側（非公開側は `missing_docs_in_private_items`）
  - `elided_lifetimes_in_paths` は借用を持つ型を `Foo<'_>` と書かせる。`Foo` と書けると「その型が借用を持つか」が字面から消え、宣言まで遡らないと読めない（G1 のライフタイムへの適用）
  - `unsafe_op_in_unsafe_fn` は Edition 2024 の既定と同じ水準を設定として固定するもので、`unsafe fn` の本体でも `unsafe {}` を書かせる
- `unsafe {}` には直前の行に `// SAFETY:` コメントが必須（`undocumented_unsafe_blocks`）。間に別の文を挟むと検出されないので、ブロックの直上に置く
- `unsafe {}` 1 つに unsafe 操作は 1 つだけ（`multiple_unsafe_ops_per_block`）。複数の操作をまとめず、操作ごとにブロックを分けて各々に `// SAFETY:` を書く（どの操作のどの前提が根拠かを 1 対 1 で対応させる）
- `// SAFETY:` は unsafe ブロック・`unsafe` 項目の直上にだけ書く（`unnecessary_safety_comment`）。unsafe を含まない式・文・宣言に付いた SAFETY コメントは「ここに検証済みの unsafe がある」という誤読を招くだけの死んだ注釈なので落とす（「有効化されていない lint を allow しない」と同じ理由）。unsafe ブロックの前後で周辺コードの前提を説明したい場合は `// SAFETY:` 以外の語（`// NOTE:` 等）で書き、`undocumented_unsafe_blocks` が要求する SAFETY と 1 対 1 の対応を崩さない
- `mod.rs` を作らない（`mod_module_files`）。サブモジュールを持つモジュールは親を `foo.rs`・子を `foo/<child>.rs` に置く（「モジュール構成」節）。統合テストの `tests/common/mod.rs` はこの lint の検査対象外なので、例外はそのまま置ける
- `map_err(|_| ...)` で元のエラーを黙って捨てない（`map_err_ignore`）。低水準の cause は leaf diagnostic の `#[source]` に入れて包み、`ParseFloatError` のように値を持たず本当に捨ててよい場合は `let ... else` / `ok_or_else` で書く（捨てていることが字面に出る形にする）
- `HashMap` / `HashSet` を `for` で直接反復しない（`iter_over_hash_type`）。`RandomState` の反復順はプロセスごとに変わるので、そのまま出力順・採番順・配列添字にすると成果物が非決定になる。順序が要る場合は `BTreeMap` を使うか、キーで `sort` した `Vec` に落としてから反復する（「集約するかは……表示順は入力の論理順」と同じ理由）。この lint が見るのは `for` だけなので、`values().sum()` のように順序に依存しない集計はイテレータアダプタで書けば発火しない — 逃げ道として使うのではなく、「順序に依存していない」と言い切れるときだけそう書く
- `Result` を `.ok()` で捨てない（`unused_result_ok`）。`let _ = f().ok();` / `f().ok();` は「`Option` に変換した」という体裁で失敗を握り潰す書き方なので、無視してよい失敗なら `let _ = f();` と書いて捨てていること自体を字面に出す（`map_err_ignore` と同じ理由）。値が要る場合は `.ok()` のまま `?` / `unwrap_or` へ繋げてよく、この lint は結果を使わない場合だけ発火する
- `-> Result` を返す関数の中で panic しない（`panic_in_result_fn`）。返り値の型で失敗を広告しておきながら本体で落ちるのは、呼び出し側から見えない失敗経路を作る書き方なので、入力（ソース・設定ファイル）由来で到達しうる状態は miette 診断エラーにして `Err` で返す（`error-handling` skill）。発火するのは `panic!` / `todo!` / `unimplemented!` と `assert!` 系で、**`unreachable!` と `debug_assert!` は発火しない** — 「必須ルール」5（`unreachable!` は積極的に使う）はこの lint と衝突せず、`-> Result` の関数でも上流の検証が保証する分岐は `unreachable!` で落としてよい。`missing_assert_message` と違ってこの lint は `#[cfg(test)]` の中でも発火するので、`?` を使うために `-> Result` を返すテスト関数の中では `assert!` ではなく `unwrap` / `expect` で落とす（既存のテストは `-> ()` なのでそのままでよい）
- `Rc` / `Arc` の複製は `Rc::clone(&x)` / `Arc::clone(&x)` と関連関数形で書く（`clone_on_ref_ptr`）。`x.clone()` は「参照カウントを増やしただけ」なのか「中身を deep copy した」のかが字面で区別できず、型宣言まで遡らないと読めない（G1「字面だけで構造が一意」の import 以外への適用）。関連関数形なら共有が呼び出し箇所に出るうえ、`Arc<Vec<u8>>` のように内側も `Clone` な型でうっかり重い複製を書いてしまった場合の差も字面に出る。`rc::Weak` / `sync::Weak` も対象（`Weak::clone(&x)` と書く）。既存コード（`project::filesystem` の読込キャッシュ・`project::font` のフォントバイト列）はすでにこの形なので、lint はその作法を固定するもの
- そのまま move すれば済む値を `clone()` しない（`redundant_clone`）。複製した先も元も、その後どこからも読まれないなら複製は無駄なだけでなく、「ここは所有権を分ける必要がある」という誤った合図を残す — 元をそのまま move すれば、値が 1 つしかないことが字面に出る（`clone_on_ref_ptr` と同じ「複製の意味を字面に出す」方向）。nursery lint だが、公式の Known problems は **false-negative**（解析が保守的で限定的）であって false-positive ではない — 発火したら真陽性として素直に直すのが既定で、`#[allow]` で外すのは考えていない。裏返すと**この lint が通ったことは「無駄な `clone` が無い」証明にはならない**（借用の絡む複製や、閉じたスコープを越える複製は見逃す）ので、`clone` を書く判断そのものは人がやる。MIR ベースの lint なのでマクロ展開後のコードにも効き、`#[cfg(test)]` の中でも発火する
- `Eq` を導出できる型に `PartialEq` だけを derive しない（`derive_partial_eq_without_eq`）。`PartialEq` 止まりは「この型の等価は部分的（`a == a` が成り立たない値がある）」という主張になるため、実際には全等価な型に付けると、利用側が「NaN のような例外値があるのか」を型宣言まで遡っても判断できない（`clone_on_ref_ptr` と同じ「意味を字面に出す」方向）。nursery lint なのは、将来 float フィールドが増えうる型や、`Eq` の追加が公開 API の約束になる型では意図的に `PartialEq` 止まりにする選択があるためだが、seiran は**`Eq` を導出できる型には例外なく `Eq` を足す**方針を採る（`PartialEq` 止まりで残っているのは f32 / f64 を持つ型だけで、そこでは lint が発火しない）。`Length` は sp（`1/65536pt` の整数）なので、座標を持つ型でも float を経由しない限りこの方針に乗る。derive の並びは `PartialEq, Eq` の順に書く（`Debug, Clone, Copy, PartialEq, Eq, Hash`）
- lint の抑制は `#[expect(...)]` + `reason = "..."` だけを使う（`allow_attributes` / `allow_attributes_without_reason`）。`allow` は抑制対象が消えても黙って残るが、`expect` なら rustc の `unfulfilled_lint_expectations` が「効いていない抑制」を落とすので、**有効化されていない lint を抑制しない**・**抑制対象が消えたら属性も消す**が機械的に守られる。`reason` に書くのは「なぜ許してよいか」＝上流のどの保証・どの設計判断が根拠かで、lint 名の言い換え（`reason = "truncation を許す"`）は書かない（`missing_assert_message` / `unreachable!` と同じ書き方）。根拠が言えない属性は書き足さずに素直に直す（`dead_code` は本当に未使用なら削除する）
- 本体ビルドでだけ発火する lint（`#[cfg(test)] mod tests` だけが使う項目に対する `dead_code` / `unused_imports`）は `#[cfg_attr(not(test), expect(...))]` と書く。素の `#[expect]` はテストビルドで充足せず `unfulfilled_lint_expectations` に落ちるため、`allow` へ戻すのではなく「本体ビルドでだけ抑制する」ことを字面に出す
- `result_large_err` は workspace で allow にしている（`#[expect]` を関数ごとに書かない）。診断 enum は miette の位置情報（`NamedSource` + `SourceSpan`）を運ぶので必ず大きくなり、これは「thiserror + miette の富んだ診断を採る」という**種類**の判断 1 個の帰結だから（個別関数の判断ではない。P10 の判定テストと同型）。逆に `cast_*` 系・`too_many_arguments`・`ref_option` は箇所ごとに根拠が違うので per-site の `#[expect]` のまま
- 本体コードの `assert!` / `debug_assert!` 系にはメッセージが必須（`missing_assert_message`）。条件の言い換えではなく「なぜ成り立つはずか」＝上流のどの保証が破れたのかを日本語で書く（`unreachable!` と同じ書き方）。テストコード（`#[test]` / `#[cfg(test)]`）では発火しないので、テストの素の `assert_eq!` はそのままでよい
- 上記のほかに、**発火 0 件の予防 lint** をまとめて有効化している（一覧と 1 行根拠は Cargo.toml のコメントが正典）。理由クラスは 5 つ — デバッグ残骸（`dbg_macro` / `todo` / `unimplemented`）/ 資源・異常終了（`exit` / `mem_forget` / `rc_mutex`）/ 既に守られている作法の lock-in（`error_impl_error` / `unnecessary_safety_doc` / `get_unwrap` ほか）/ 対立 pair の現行スタイル側（`pub_without_shorthand` / `semicolon_inside_block`）/ ライフタイム表記の残骸（rustc の `redundant_lifetimes` / `single_use_lifetimes` / `trivial_numeric_casts` / `unused_lifetimes`）。**新しい lint を足すときは「一括 sweep で 0 に見えた」を根拠にしない** — lint 単位で `cargo clippy --all-targets --all-features --message-format=json -- -W clippy::<name>` を回し、診断コードで数える（短縮フォーマットの grep は lint 名を含まないので 0 件に見える）
- `println!` / `eprintln!` はライブラリ 2 crate では使わない（`print_stdout` / `print_stderr`）。表示はユーザーへ届ける成果物なので CLI（`seiran`）の crate root だけ `#![expect]` で開けてある。ライブラリ側で人へ伝えたいことは診断（miette）か tracing に載せる（#377 の役割分担）
- CI と pre-commit フックは `cargo clippy --all-targets --all-features -- -D warnings` で走る。warn レベルの指摘もそこでビルド失敗になるため、素の `cargo clippy` ではなくこの形で確認する
- 本体コードで `.unwrap()` を使わない（`unwrap_used`）。`.expect("...")` に移し、メッセージには条件の言い換えではなく「なぜ落ちないか」＝上流のどの検証・型設計が保証するかを日本語で書く（`unreachable!` / `missing_assert_message` と同じ書き方）。到達しうる失敗（ソース・設定ファイル由来）なら `.expect()` ではなく miette 診断エラーにする（`error-handling` skill）。`#[test]` / `#[cfg(test)]` の中は clippy.toml の `allow-unwrap-in-tests = true` で対象外だが、`tests/` から使うヘルパ（`#[doc(hidden)] pub` の `test_support` 等）は cfg(test) の外なので本体と同じ扱いになる
- `expect_used` は**有効化していない**（restriction lint で `all` にも `pedantic` にも含まれない）。`.expect()` は「根拠を書いた上での逃げ道」として残してある。したがって `#[expect(clippy::expect_used)]` は何も抑制せず、書けば `unfulfilled_lint_expectations` で落ちる

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei` ファイル群）、フォント: リポジトリ直下の `fonts/`
- AAA パターンで記述し、`// Arrange` / `// Act` / `// Assert` コメントで区切る
- **共有ヘルパ**: 3 つ以上の test module が同じヘルパを必要としたら、各 module へ複製せず `#[cfg(test)]` で閉じた `test_support` module に切り出して 1 箇所に集める（`frontend::evaluator::test_support` / `typeset::lowering::test_support`）。切り出し先は「そのヘルパが注入する本番の仕組みを持つ module」で、呼び出し側は `test_support::parse(...)` のように module 経由で呼ぶ。crate 外の統合テスト（`tests/`）も使うヘルパだけは例外で、`#[cfg(test)]` では閉じられないので `#[doc(hidden)] pub mod` として root facade に載せる（`project::config::test_support` → `seiran_compiler::test_support`）
- test module も本体と同じ use 規約に従う（「必須ルール」3）。親の被テスト項目を `use super::*` / `use super::Item` で取り込むのは許容だが、それ以外は `crate::` 起点で import する
- テストコードでは `unwrap` / `expect` を許容する（`unwrap_used` は clippy.toml の `allow-unwrap-in-tests` で対象外、`expect_used` は無効なので、どちらも属性は付けない）。`expect` のメッセージは日本語で期待を書く（例: `"一時ファイルを作成できるはず"`）。ただし `tests/` から使うヘルパは cfg(test) の外なので `unwrap_used` が効く（本体と同じく `.expect()` + 根拠）
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

上記はすべて本体スレッド限定 — **サブエージェントでは LSP / ToolSearch は使えない**（ToolSearch はセッション全体で subagent に無効、LSP は deferred tool なので subagent からスキーマを読めない）。シンボル追跡が要る調査は本体スレッドが LSP で行い、explore agent は「ここから先は LSP が必要」と報告で返す（報告規約は `.claude/agents/explore.md`）。agent 定義の `tools:` には実在するツール名だけを列挙する — 未知名を含めると許可リストのパース自体が壊れ、可視ツールが縮退する。

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                                                                                           |
| ----------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ（`[pdf]` の width / height）、`[pdf].show_bookmarks`（しおり出力）、`[image]`（画像 DPI / downsample）、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語 |
| `style.toml`                        | **見た目**                 | 本文領域のページ内側余白（`[page]` の margin_top / bottom / left / right）、見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、脚注の体裁と採番方式、段組み数、参照リンク色                                         |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                                                                                 |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）
- **値の基本書式**: 長さ（`Length`）は単位付き文字列 `"12pt"` / `"5mm"`（素の数値は不可）、色（`Color`）は `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問、`[r, g, b]` 配列は不可）
- **style.toml の詳細スキーマ**（キャプションと番号 3 系統・見出し 2 レイヤーマージ・カウンタ固定 9 種・`[math.script]` / `[math.block]`・`[page]` の余白と `flush_bottom` 等）は `docs/architecture.md` の `style` 節を参照

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`

## issue / PR 運用

issue・PR・branch・commit・ラベル・sub-issue の運用規約は `issue-pr-ops` skill に集約。
GitHub 上で issue / PR を作る・編集する、branch を切る、commit メッセージや merge 方法を決める、
ラベルや epic / sub-issue の親子関係を判断する際は、その skill を参照すること。
クレート構成・パイプライン・設定スキーマ・CLI に触れる PR を仕上げる際は
`docs-sync` skill のチェックリストでドキュメント更新漏れを確認すること。
