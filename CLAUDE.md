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
   - **`crate::` を本体コードへ直書きしない**: 型・トレイトは import して裸の名前で書き、関数は import した module 経由で `module::fn(...)` と呼ぶ。`clippy::absolute_paths` は 4 セグメント以上しか検出しない（`clippy.toml` の `absolute-paths-max-segments = 3`。末尾の enum variant / 関連関数を数えないので `crate::source::SourceId::new` は素通りし、`project::config::load` と `style::load` を書き分けるイディオムは温存される）ので、規約のほうが lint より厳しい。短いパス側は rustc の `unused_qualifications` が押さえていて、スコープに入っている名前を `std::sync::Arc::new` のように再修飾すると落ちる。どちらの lint も `#[cfg(test)]` の中で発火するので、テストにも同じ規約が効く。doc コメント内の intra-doc link（``[`crate::Foo`]``）は絶対パスが正しいので対象外
   - `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`
   - 型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい
   - `#[cfg(test)]` の項目だけが使う import は `#[cfg(test)] use ...;` と書いて本体ビルドから外す（本体の use ツリーへ混ぜると非テストビルドで unused になる）
4. **ドキュメントコメント**: すべてのモジュール・型（struct / enum / trait）・関数に **日本語** で記述（`missing_docs_in_private_items` と rustc の `missing_docs` の 2 枚で全項目の**有無だけ**が検査される。日本語で書かれているかは検査されないので人が見る）
5. **`unreachable!` は積極的に使う**: まず型設計で到達不能な状態自体を表現不能にできないか検討し、それでも残る「絶対に到達しない」分岐は `_ => {}` / `Default::default()` / 黙って `Ok` を返す等でごまかさず `unreachable!` で落とす（不変条件の破れを最寄りで顕在化させる）。ただし入力（ソース・設定ファイル）由来で到達しうる状態は panic ではなく miette 診断エラーにする（`error-handling` skill 参照）。本体コードでは「なぜ到達しないか」＝上流のどの検証が保証しているかをメッセージに書く（例: `unreachable!("許可リスト外は strict_command_calls がエラーにする")`。テストの let-else 分解など自明な箇所は省略可）
6. **panic は根拠を書いてから落とす**: 本体コードで `.unwrap()` を使わず `.expect("...")` に移し、メッセージには条件の言い換えではなく「なぜ落ちないか」＝上流のどの検証・型設計が保証するかを日本語で書く（`unwrap_used`。`unreachable!` / assert メッセージと同じ書き方）。同じ根拠が何箇所にも並ぶなら、根拠ごとヘルパ関数へ畳んで 1 箇所にする（`frontend::syntax::parser` の `take_peeked` / `peeked`）。`assert!` / `debug_assert!` 系にもメッセージが必須（`missing_assert_message`。テストコードでは発火しないので素の `assert_eq!` でよい）。`-> Result` を返す関数の中では panic しない（`panic_in_result_fn`）— 入力由来で到達しうる状態は miette 診断エラーにして `Err` で返す。この lint が見るのは `panic!` / `todo!` / `unimplemented!` と `assert!` 系で、**`unreachable!` と `debug_assert!` は発火しない**ので必須ルール 5 と衝突しない。`-> Result` の外でも本体コードに裸の `panic!` は書かない（`panic`）— 到達しないなら `unreachable!`、入力由来なら miette 診断で、どちらでもない `panic!` は残らないため。テストは `allow-panic-in-tests` で対象外だが、`tests/` から使うヘルパは `#[cfg(test)]` の外なので発火する（`unwrap_used` と同じ）。`#[cfg(test)]` の中でも発火するため、`?` のために `-> Result` を返すテスト関数では `assert!` ではなく `unwrap` / `expect` で落とす。その `unwrap` / `expect` の側は `unwrap_in_result` が見る — `Result` / `Option` を返す関数の中で**返り値と同じ型ファミリ**の `.unwrap()` / `.expect()` を書いたら、`#[expect(clippy::unwrap_in_result, reason = ...)]` を関数に付けて例外だと宣言する（この lint は `allow-unwrap-in-tests` を見ないので `#[cfg(test)]` の中でも発火し、上の「`-> Result` を返すテスト関数は `unwrap` / `expect` で落とす」もこの属性とセットになる。関数単位なので中の複数箇所を 1 枚で覆える）。**この lint は網羅ではない** — 型ファミリが食い違う組（`-> Result` の中の `Option::expect`、`-> Option` の中の `Result::expect`）とクロージャの本体は発火しないので、属性が無いことは「根拠を検討済み」を意味しない（`missing_docs*` が doc の有無だけを見て日本語かは見ないのと同じ。根拠を書く責任は lint ではなく本ルールの側にある）。`reason` に書けるのは「panic が必要」か「panic し得ない」のどちらかで、どちらも書けないなら属性ではなく診断エラー化かリファクタで直す。`.expect()` 自体は「根拠を書いた上での逃げ道」として残してあり、`expect_used` は有効化していない
7. **unsafe は 1 操作 1 ブロック 1 SAFETY**: `unsafe {}` 1 つに unsafe 操作は 1 つだけ置き（`multiple_unsafe_ops_per_block`）、その直上の行に `// SAFETY:` を書く（`undocumented_unsafe_blocks`。間に別の文を挟むと検出されない）。「どの操作のどの前提が根拠か」を 1 対 1 で対応させるためで、逆に unsafe を含まない箇所へ SAFETY コメント・`# Safety` doc を書かない（`unnecessary_safety_comment` / `unnecessary_safety_doc`。「ここに検証済みの unsafe がある」という誤読を招く死んだ注釈になる）。周辺コードの前提を説明したいときは `// NOTE:` 等の別の語で書く。`unsafe fn` の本体でも `unsafe {}` を書く（`unsafe_op_in_unsafe_fn`）

### モジュール構成

- **`mod.rs` を使わない**（`mod_module_files`）: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

  例外: 統合テスト（`tests/`）の共通ヘルパは慣例どおり `tests/common/mod.rs` に置く（`common.rs` だとテストファイルとして扱われるため。この lint の検査対象外なのでそのまま置ける）。

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` / `pub(crate) mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `project::config` は入口を `project::config::load` と読ませて `style::load` と区別する。crate root 直下の非公開 module は crate 全体から到達できるため、garde カスタムバリデータを持つ `length` に `pub(crate)` は不要）。同名の型を 2 つ作って module 公開で回避しない — 名前側を変えて衝突自体を無くす（例: `ConfigValidationError` / `StyleValidationError`）。root facade へ載せるのは実際に名指しされる名前だけで、内部フィールド型としてしか現れない名前は再エクスポートしない（この 2 方向は rustc の `unreachable_pub` と `unnameable_types` が機械化している — 外から到達しない `pub` は狭め、公開シグネチャに現れるのに facade から名指しできない型は facade へ出すか宣言を狭める）。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### 値と型の書き方

字面から意味が読めることを優先する（G1 のコードへの適用）。以下は末尾の enum match の項を除き lint が機械化している。

- `Rc` / `Arc` の複製は `Rc::clone(&x)` / `Arc::clone(&x)` と関連関数形で書く（`clone_on_ref_ptr`）。`x.clone()` は「参照カウントを増やしただけ」なのか「中身を deep copy した」のかが字面で区別できず、型宣言まで遡らないと読めない。`rc::Weak` / `sync::Weak` も対象
- そのまま move すれば済む値を `clone()` しない（`redundant_clone`）。複製先も元もその後読まれないなら、複製は無駄なだけでなく「ここは所有権を分ける必要がある」という誤った合図を残す。nursery lint だが Known problems は **false-negative**（解析が保守的）なので、発火したら真陽性として直す。裏返すと**この lint が通っても「無駄な `clone` が無い」証明にはならない**ので、`clone` を書く判断そのものは人がやる
- `Eq` を導出できる型に `PartialEq` だけを derive しない（`derive_partial_eq_without_eq`）。`PartialEq` 止まりは「等価が部分的（`a == a` が成り立たない値がある）」という主張になるので、実際には全等価な型に付けると利用側が「NaN のような例外値があるのか」を判断できない。`PartialEq` 止まりで残るのは f32 / f64 を持つ型だけ（そこでは lint が発火しない）。`Length` は sp（1/65536pt の整数）なので float を経由しない限りこの方針に乗る。derive の並びは `Debug, Clone, Copy, PartialEq, Eq, Hash` の順
- 公開型には `Debug` を実装する（`missing_debug_implementations`）。derive で生バイト列が出力に載る型（読込キャッシュ・フォント・画像）は手書きにして件数・長さだけを出す（`publication` / `project::filesystem` の `Debug`）
- 借用を持つ型は `Foo<'_>` と書く（`elided_lifetimes_in_paths`）。`Foo` と書けると借用の有無が字面から消え、宣言まで遡らないと読めない
- 型推論で足りる `as` は書かない（`trivial_casts`）。trait object から auto trait（`Send` / `Sync`）を落とす変換のように**外すとコンパイルが通らない**キャストで発火することがあり、そこだけ `#[expect]` + 理由で残す（`compiler::compile_failure`）
- 数値リテラルの型サフィックスは `1u32` 形（`separated_literal_suffix`）。`1_u32` 形と混在させない
- 識別子は ASCII で書く（`non_ascii_idents`）。テスト名も同じで、日本語は doc コメント・診断メッセージ・assert の文言に置き、名前には持ち込まない
- enum を match するときは「入力 enum に variant を追加したら、この処理の判断を必ず見直すべきか」で wildcard の可否を決める。**Yes なら wildcard を使わず全 variant を明示する**（同じ結果になる variant は `|` でまとめる）— enum 間・enum から値への意味的な対応表、parser / evaluator の dispatch、状態遷移、段間語彙の完全走査、新しい variant がアルゴリズムへ参加するかを必ず判断すべき分類がこれにあたり、明示しておくと variant 追加が見直すべき箇所でコンパイルエラーになる。**No なら wildcard を維持する** — ある variant だけを取り出す抽出・検索、明示された部分集合だけを判定する述語、許可リスト以外を同じ診断にする既定エラー、非対象値を変更せず通す pass-through、enum 自身の共通操作へ委譲する分岐では「新しい variant も既定へ入る」ことが処理の意味なので、網羅化すると意味が壊れる。`clippy::wildcard_enum_match_arm` はこの Yes / No を区別できず両方を等しく報告するので有効化しない（#402 / #426）— **この項だけは lint ではなく人が守る**。ガード付き arm と網羅性判定の関係に注意する（`Penalty { .. } if cond` のようなガード付き arm は網羅性判定に参加しないので、同じ variant を wildcard 側の列挙にも書く。逆に `Glue { breakable: true, .. }` のようなリテラルパターンは参加する）

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
- `map_err(|_| ...)` で元のエラーを黙って捨てない（`map_err_ignore`）。低水準の cause は leaf diagnostic の `#[source]` に入れて包み、`ParseFloatError` のように値を持たず本当に捨ててよい場合は `let ... else` / `ok_or_else` で書く（捨てていることが字面に出る形にする）
- `Result` を `.ok()` で捨てない（`unused_result_ok`）。無視してよい失敗なら `let _ = f();` と書いて捨てていること自体を字面に出す。値が要る場合は `.ok()` のまま `?` / `unwrap_or` へ繋げてよく、この lint は結果を使わない場合だけ発火する
- 診断 enum が大きいこと（`result_large_err`）は workspace の allow で畳んである。miette の位置情報を運ぶ以上どの診断も大きくなり、これは「thiserror + miette の富んだ診断を採る」という**種類**の判断 1 個の帰結だから（P10 の判定テストと同型）。逆に `cast_*` 系・`too_many_arguments`・`ref_option` は箇所ごとに根拠が違うので per-site の `#[expect]` のまま
- ライブラリ 2 crate では `println!` / `eprintln!` を使わない（`print_stdout` / `print_stderr`）。人へ伝えたいことは診断（miette）か tracing に載せる。表示はユーザーへ届ける成果物なので、CLI（`seiran`）の crate root だけ `#![expect]` で開けてある
- 決定性が要る場所で `HashMap` / `HashSet` を `for` 反復しない（`iter_over_hash_type`）。`RandomState` の反復順はプロセスごとに変わるので、出力順・採番順・配列添字にすると成果物が非決定になる。`BTreeMap` を使うか、キーで `sort` した `Vec` に落としてから反復する。この lint が見るのは `for` だけなので `values().sum()` のような順序非依存の集計は発火しない — 逃げ道として使うのではなく、「順序に依存していない」と言い切れるときだけそう書く

### Clippy

**lint の採用根拠の正典は root `Cargo.toml`** — `[workspace.lints.clippy]` / `[workspace.lints.rust]`（各クレートは `lints.workspace = true` で両テーブルを継承）に 1 lint = 1 行の根拠コメントが付いていて、節見出しが下の 6 軸に対応する。lint 側の設定値は root `clippy.toml`（`absolute-paths-max-segments = 3` / `allow-unwrap-in-tests = true` / `allow-panic-in-tests = true`）。**ここには原理と運用だけを置く** — lint の増減と根拠の更新が同じ diff に閉じるように、個々の lint の理由は Cargo.toml のコメントに書く（個々の lint が要求する**書き方**は上の規約各節に置く）。

#### 選定の 6 軸

| 軸 | 何を採るか | 例 |
| --- | --- | --- |
| A 規約の機械化 | prose の規約に対応する lint（最優先） | `implicit_return` / `missing_docs*` / `mod_module_files` / SAFETY 族 |
| B 字面に意味 | G1「字面だけで構造が一意」のコードへの適用 | `clone_on_ref_ptr` / `map_err_ignore` / `elided_lifetimes_in_paths` |
| C 決定性 | 成果物のバイト再現性を壊すものを拒む | `iter_over_hash_type` / `float_cmp_const` |
| D 0 件予防 | 発火 0 件かつ規約整合＝無料の再発防止 | `dbg_macro` / `exit` / `rc_mutex` |
| E 対立 pair の lock-in | 2 通り書ける形は現行スタイル側（0 件の側）を固定する | `separated_literal_suffix` / `pub_without_shorthand` |
| F nursery 個別主義 | Known problems を理解した lint だけを 1 件ずつ採用する | `redundant_clone` / `fallible_impl_from` |

A〜C は「目的」、D〜F は「採用条件」の軸で、1 つの lint が両方の性質を持つことはある（`float_cmp_const` は C かつ発火 0 件）— Cargo.toml の節見出しには採用の決め手になった軸を置く。軸に載らない lint は採らない。lint が**発火＝提案に従う**とは限らない — `suboptimal_flops` / `imprecise_flops` は「float の合成演算をここで書いた」ことの通知として有効化してあり、採否は箇所ごとに決める（項を累積して精度が効くなら `mul_add` / `ln_1p` を採る・値が一度動くので golden 再生成込み／1 項で定数畳み込みや可読性が勝つなら積へ名前を付けて式から乗算を外す／どちらでもないなら `#[expect]` + 根拠）。`mul_add` は IEEE の単一丸めで `a * b + c` と同じく決定的なので、C 軸の懸念は移行時の値の変化に限られる。`clippy::all` が deny、`pedantic` が warn で、group を個別 lint が上書きする（group は priority -1、個別は既定の 0）。

#### 運用

- CI と pre-commit フックは `cargo clippy --all-targets --all-features -- -D warnings` で走る。warn レベルの指摘もそこでビルド失敗になるため、素の `cargo clippy` ではなくこの形で確認する
- **抑制は `#[expect(...)]` + `reason = "..."` だけ**（`allow_attributes` / `allow_attributes_without_reason`）。`allow` は抑制対象が消えても黙って残るが、`expect` なら rustc の `unfulfilled_lint_expectations` が「効いていない抑制」を落とすので、**有効化されていない lint を抑制しない**・**抑制対象が消えたら属性も消す**が機械的に守られる。`reason` には「なぜ許してよいか」＝上流のどの保証・どの設計判断が根拠かを書き、lint 名の言い換え（`reason = "truncation を許す"`）は書かない。根拠が言えない属性は書き足さずに直す（`dead_code` は本当に未使用なら削除する）
- 本体ビルドでだけ発火する lint（`#[cfg(test)] mod tests` だけが使う項目に対する `dead_code` / `unused_imports`）は `#[cfg_attr(not(test), expect(...))]` と書く。素の `#[expect]` はテストビルドで充足せず `unfulfilled_lint_expectations` に落ちるため、`allow` へ戻すのではなく「本体ビルドでだけ抑制する」ことを字面に出す
- **新しい lint の 0 件は lint 単位で実測する**。`cargo clippy --all-targets --all-features --message-format=json -- -W clippy::<name>` を回し、診断コード（`message.code.code`）で数える。短縮フォーマットの grep は出力に lint 名を含まないため全件 0 に見える（この偽陰性で 14 件が隠れた実例がある）
- 採らないと決めた lint の理由は #402 に記録がある（恒久不採用と、条件が変われば再検討するものを分けてある）

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei` ファイル群）、フォント: リポジトリ直下の `fonts/`
- AAA パターンで記述し、`// Arrange` / `// Act` / `// Assert` コメントで区切る。テスト名に `test_` 接頭辞は付けない（`redundant_test_prefix`）— 何を検証するかだけを書く
- **共有ヘルパ**: 3 つ以上の test module が同じヘルパを必要としたら、各 module へ複製せず `#[cfg(test)]` で閉じた `test_support` module に切り出して 1 箇所に集める（`frontend::evaluator::test_support` / `typeset::lowering::test_support`）。切り出し先は「そのヘルパが注入する本番の仕組みを持つ module」で、呼び出し側は `test_support::parse(...)` のように module 経由で呼ぶ。crate 外の統合テスト（`tests/`）も使うヘルパだけは例外で、`#[cfg(test)]` では閉じられないので `#[doc(hidden)] pub mod` として root facade に載せる（`project::config::test_support` → `seiran_compiler::test_support`）
- test module も本体と同じ use 規約に従う（「必須ルール」3）。親の被テスト項目を `use super::*` / `use super::Item` で取り込むのは許容だが、それ以外は `crate::` 起点で import する
- テストコードでは `unwrap` / `expect` / `panic!` を許容する（`unwrap_used` / `panic` は clippy.toml の `allow-unwrap-in-tests` / `allow-panic-in-tests` で対象外、`expect_used` は無効なので、いずれも属性は付けない）。`expect` のメッセージは日本語で期待を書く（例: `"一時ファイルを作成できるはず"`）。ただし `tests/` から使うヘルパは cfg(test) の外なので `unwrap_used` / `panic` が効く（本体と同じく `.expect()` + 根拠。エラー内容の整形が必要で `.expect()` の Debug では足りない箇所だけ `#[expect(clippy::panic, reason = ...)]`）。`unwrap_in_result` だけは例外で `allow-*-in-tests` 相当の設定を持たず `#[cfg(test)]` の中でも発火するので、`Result` / `Option` を返すテストヘルパで返り値と同じ型ファミリを `unwrap` したら `#[expect(clippy::unwrap_in_result, reason = ...)]` を付ける（`frontend::evaluator::environment::table` の `eval_table` / `project::config` の `run_validate_with_serif_extra`）
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
