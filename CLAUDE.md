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
cargo run -- build [-c <config_path>] [-v|-vv|-q]        # 設定ファイルの sources から PDF を生成（-v 工程 / -vv 内部詳細 / -q 抑止）
cargo run -- variation-axes <font> [-f <font_index>]       # バリアブルフォント軸情報を表示
cargo run -- ttc-names <ttc_file>                          # TTC ファイル内のフォント名一覧を表示
cargo run -- script-langs <font> [-f <font_index>]         # サポートされるスクリプト / 言語を表示
cargo +nightly fmt                                         # フォーマット（nightly 必須）
cargo clippy --all-targets --all-features -- -D warnings   # リント（CI / pre-commit と同じ形）
cargo test --all-features                                  # テスト実行（doctest 込み。CI / pre-commit と同じ形）
cargo test -p <crate_name>                                 # 特定クレートのテスト実行
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --all-features   # doc ビルド（CI と同じ形）
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
                       シェーパー）と内部順序（画像読込・寸法確定 → lowering → (a) boxing
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
- 数式は閉じた箱（`HBoxContent::Atom`）として行分割をまたがない。記号間のアキは数式クラスの表から
  固定 kern（1mu = font_size/18）で出し、ソースに書かれた空白は組版に出さない
- 脚注は本文の実効下限を縮めて配置し、行単位でページ間繰越。ページ単位採番のときだけ本文パスを
  不動点まで反復する（`typeset::pagination::footnote_numbering`）
- `compile` は PDF バイト列の生成・保存を行わない。`seiran_pdf::render` と atomic write は CLI（`seiran`）の責務

### クレート構成

crate はデプロイ・外部依存・独立再利用の単位に限る（コンパイル段階を crate 境界にしない）。
描画バックエンドが 1 つである間は `Renderer` trait も共有型だけの第三 crate も作らない（#372）。

```text
seiran-compiler    言語処理・意味解決・組版のライブラリ（lib target のみ）。組版成果物
                   （`Publication` 系 leaf 型）の型所有者。公開 API は compile + 成果 Compilation
                   （Publication / DependencyManifest / Warnings / BuildStatistics / pdf_path）
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
| `publication` | 組版成果物の確定表現（`Publication` / `PaintOp` / 描画資源）と、その唯一の構築経路。krilla を知らない純データ |
| `compiler` | compile facade。全体の phase 順序だけを持ち、組版中間型の走査・描画資源の構築を名指ししない |

## コーディング規約

正典は **`docs/coding-conventions.md`**（各規約の根拠・lint の検出範囲・境界事例）。ここには規約の本文と、
編集中に効く落とし穴だけを置く。lint の採用根拠は root `Cargo.toml` のコメント（1 lint = 1 行）、設定値は
`clippy.toml`、エラー型・診断の設計は `error-handling` skill、組版の検証は `verify-typesetting` skill。

### 必須ルール（番号は Cargo.toml のコメントが参照する — 固定）

1. **`return` キーワード必須**: 末尾式による暗黙の返却は使わない（`implicit_return`）
2. **フォーマット**: 正典は `rustfmt.toml`（2 スペース・120 桁）。適用は `cargo +nightly fmt`（nightly 必須）
3. **use 文**: 持ち込んだ名前の出自が呼び出し箇所の字面で一意に分かる範囲で最短パスを使う。起点は `crate::` に統一し `super::` / `self::` は使わない — 例外は (a) 同じファイルが `mod` 宣言する子 module からの相対 use、(b) `#[cfg(test)]` module が**直近の親**を `use super::` で取り込む形（`super::super::` は不可）の 2 つだけ。`crate::` を本体コードへ直書きしない（型・トレイトは裸の名前、関数は `module::fn(...)`。規約は `absolute_paths` / `unused_qualifications` より厳しく、テストにも効く。doc コメントの intra-doc link ``[`crate::Foo`]`` は絶対パスが正しいので対象外）。`*` を避け明示 import、型・トレイト・モジュールは直接 import、関数は既定でモジュール経由（出自が自明な慣用は直接可）。`#[cfg(test)]` だけが使う import は `#[cfg(test)] use ...;`
4. **ドキュメントコメント**: すべてのモジュール・型・関数に**日本語**で（`missing_docs*` は有無だけ検査。日本語かは人が見る）
5. **`unreachable!` は積極的に使う**: 型で表現不能にできない「絶対に到達しない」分岐は `_ => {}` / `Default::default()` / 黙って `Ok` でごまかさず `unreachable!`。入力（ソース・設定）由来で到達しうる状態は miette 診断エラー。メッセージには「なぜ到達しないか」＝上流のどの検証が保証するかを書く
6. **panic は根拠を書いてから落とす**: 本体で `.unwrap()` は使わず `.expect("なぜ落ちないか")`（条件の言い換えは不可。同じ根拠が並ぶならヘルパへ畳む）。`assert!` 系にもメッセージ必須（テストでは発火しないので素の `assert_eq!` でよい）。`-> Result` の中では panic しない（`panic_in_result_fn`。`unreachable!` / `debug_assert!` は対象外）。裸の `panic!` は書かない（`panic`。`tests/` から使うヘルパは cfg(test) 外なので効く）。`-> Result` / `-> Option` の関数で返り値と同じ型ファミリを `unwrap` / `expect` したら `#[expect(clippy::unwrap_in_result, reason = ...)]` — テストの中でも発火する。`reason` に書けるのは「panic が必要」か「panic し得ない」だけで、どちらも書けないなら診断エラー化かリファクタ。lint は網羅ではないので属性が無い ≠ 根拠検討済み
7. **unsafe は 1 操作 1 ブロック 1 SAFETY**: `unsafe {}` 1 つに操作 1 つ、直上に `// SAFETY:`（間に文を挟まない）。unsafe を含まない箇所へ SAFETY コメント・`# Safety` doc は書かない（`// NOTE:` 等を使う）。`unsafe fn` の本体でも `unsafe {}`

### モジュール構成

- **`mod.rs` を使わない**: 親は `foo.rs`、子は `foo/<child>.rs`（`mod_module_files`）。例外は `tests/common/mod.rs` だけ
- **既定で非公開 + root ファサード**: 子は `mod`、公開 API は root（または親）の `pub use` で 1 本に揃える。`pub mod` / `pub(crate) mod` は module 名が名前空間として意味を持つときだけ（`project::config::load` vs `style::load`）。同名型を 2 つ作って module 公開で回避せず名前側を変える（`ConfigValidationError` / `StyleValidationError`）。facade へ載せるのは実際に名指しされる名前だけ（`unreachable_pub` / `unnameable_types`）。利用側は最浅の公開パスから import し、enum variant は import せず `Enum::Variant` と書く
- **同一ファイル内で 1 型の inherent impl を分けない**（`multiple_inherent_impl`）: ライフタイム引数の有無で分かれているだけなら名前付きの側へ寄せる。別ファイルへ切り出した impl は lint の対象外なので分割の慣行と衝突しない
- **分割の判断基準**: 行数ではなく**自己完結した本体コードの塊**の大きさ。大半がインラインテストなら分割しない。切り出すのはエラー型 enum のようにロジックを持たず private 内部に依存しない塊で、`Parser` 等の private フィールドに密結合したメソッド群は可視性を緩めてまで分割しない
- 切り出した型は親で `pub use <child>::<Type>;` して公開パスを維持するのが既定。新パスのほうが明確なら変更可

### 値と型の書き方

字面から意味が読めることを優先する（G1 のコードへの適用）。enum match と `clone` の要否以外は lint が機械化。

| 書き方 | lint |
| --- | --- |
| `Rc` / `Arc` / `Weak` の複製は `Rc::clone(&x)` の関連関数形 | `clone_on_ref_ptr` |
| 共有する読み取り専用バイト列は `Arc<[u8]>`（`Arc<Vec<u8>>` は二重間接なうえ移し替えで複製が走る。外部 API 都合は `AsRef<[u8]>` の newtype で包む） | `rc_buffer` |
| move で済む値を `clone()` しない。発火は真陽性として直す（false-negative なので通っても「無駄な clone なし」の証明にはならない — 判断は人） | `redundant_clone` |
| `Eq` を導出できる型に `PartialEq` だけを derive しない（`PartialEq` 止まりは f32 / f64 を持つ型だけ）。derive の並びは `Debug, Clone, Copy, PartialEq, Eq, Hash` | `derive_partial_eq_without_eq` |
| 公開型に `Debug`。生バイト列が載る型（読込キャッシュ・フォント・画像）は手書きで件数・長さだけ出す | `missing_debug_implementations` |
| 借用を持つ型は `Foo<'_>` | `elided_lifetimes_in_paths` |
| 型推論で足りる `as` は書かない。auto trait を落とす必須キャストだけ `#[expect]` + 理由 | `trivial_casts` |
| 個数の決まった繰り返しは `(0..n).map(\|_\| v)` ではなく `repeat_n(v, n)` / `repeat_with(..).take(n)`（副作用があるなら後者） | `map_with_unused_argument_over_ranges` |
| `PathBuf` は `clone()` + `push()` ではなく `join()` で組み立てる（拡張子だけ `set_extension`） | `pathbuf_init_then_push` |
| 数値リテラルの型サフィックスは `1u32` 形 | `separated_literal_suffix` |
| エスケープの要らない文字列に `r"…"` を付けない | `needless_raw_strings` |
| 識別子は ASCII（テスト名も）。日本語は doc・診断・assert 文言へ | `non_ascii_idents` |

- **enum match の wildcard（人が守る）**: 「入力 enum に variant を追加したら、この処理の判断を必ず見直すべきか」で決める。**Yes なら全 variant を明示**（意味的な対応表・parser / evaluator の dispatch・状態遷移・段間語彙の完全走査・新 variant がアルゴリズムへ参加するか判断すべき分類。同じ結果は `|` でまとめる）。**No なら wildcard を維持**（抽出・検索、部分集合の述語、許可リスト外を同じ診断にする既定エラー、pass-through、enum 自身の共通操作への委譲 — 「新 variant も既定へ入る」が処理の意味）。ガード付き arm は網羅性判定に参加しないので、同じ variant を wildcard 側の列挙にも書く。`wildcard_enum_match_arm` は Yes / No を区別できないので有効化しない

### エラーハンドリング・バリデーション

正典は `error-handling` skill。常時効く原則:

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語）。`miette::Result<T>` は CLI 入口だけ
- `compile` の失敗型は不透明型 `CompileFailure`（先頭が主診断・空で構築不能）。ユーザーが最初に読むメッセージは常に修正可能な leaf diagnostic
- 診断 `code` の第 1 階層は段の固定列挙（`project` / `style` / `frontend` / `semantics` / `typeset` / `compiler` / `pdf` / `cli`）、第 2 階層以降は意味的カテゴリ（module パスではない）
- 設定値検証は `garde`、違反は `Failures<E>` に集めて 1 度に報告。集約するかは「失敗後も独立な検査を安全かつ決定的に続けられるか」で決め、表示順は入力の論理順。集約自身に `code` を付けない（`Failures<E>` は `Diagnostic` 非実装で型保証）
- 外部資源の read error は低水準 cause として leaf diagnostic の `#[source]` へ。warning は error と公開型を共用せず（`Warnings`）、同じ問題を診断と tracing の両方で出さない
- `map_err(|_| ...)` で元のエラーを捨てない（`map_err_ignore`）。`Result` を `.ok()` で捨てず `let _ = f();` と書く（`unused_result_ok`）
- ライブラリ 2 crate で `println!` / `eprintln!` を使わない（`print_stdout` / `print_stderr`。CLI の crate root だけ `#![expect]`）
- 決定性が要る場所で `HashMap` / `HashSet` を `for` 反復しない（`iter_over_hash_type`）。`BTreeMap` かキーで `sort` した `Vec` へ。順序非依存の集計だけ例外
- `result_large_err` は workspace の allow（種類の判断 1 個の帰結）。`cast_*` / `too_many_arguments` / `ref_option` は箇所ごとに根拠が違うので per-site の `#[expect]`

### Clippy 運用

lint の採用根拠は root `Cargo.toml` の 1 行コメント（`[workspace.lints]` の clippy / rust / rustdoc 3 テーブルとも同じ規則）、節見出しは有効化の目的（規約の機械化 / 字面に意味 / 表記の固定 / 決定性 / crate の責務境界 / 誤りの検出 / 残骸を残さない / nursery）で、置き場の規則と採用条件は `docs/coding-conventions.md` の Clippy 節。`clippy::all` が deny、`pedantic` が warn。

- 確認は CI / pre-commit と同じ `cargo clippy --all-targets --all-features -- -D warnings`（warn もビルド失敗になる）
- rustdoc lint（`[workspace.lints.rustdoc]`）は `cargo doc` を回して初めて効く。CI と同じ形は
  `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --all-features`
- **抑制は `#[expect(...)]` + `reason = "..."` だけ**（`allow_attributes*`）。`reason` は「なぜ許してよいか」＝上流のどの保証・設計判断が根拠かで、lint 名の言い換えは不可。根拠が言えないなら直す（`dead_code` は削除）
- 本体ビルドでだけ発火する lint は `#[cfg_attr(not(test), expect(...))]`（素の `#[expect]` はテストビルドで `unfulfilled_lint_expectations` に落ちる）
- 新 lint の 0 件は `--message-format=json -- -W clippy::<name>` で診断コード単位に実測する（短縮フォーマットの grep は lint 名を含まず偽陰性）
- `suboptimal_flops` / `imprecise_flops` は通知として有効化してあり、発火＝提案に従うとは限らない（採否は箇所ごと）。不採用 lint の理由は #402（clippy 初回）/ #421（rustc）/ #473（clippy 未処分 84・`clippy.toml` ノブ・rustdoc）/ #482（第 3 次 sweep — rustdoc lint 2 種の撤回・基準に矛盾する処分 4 件の是正）

### テスト

- 入力は `tests/text/`（機能別 `.sei`）、フォントは `fonts/`
- AAA。`// Arrange` / `// Act` / `// Assert` は 3 段が実際に複数行へ分かれるテストだけ。テスト名に `test_` 接頭辞は付けない（`redundant_test_prefix`）
- 3 つ以上の test module が使うヘルパは `#[cfg(test)]` の `test_support` module 1 箇所へ（`frontend::evaluator::test_support` / `typeset::lowering::test_support`。置き場は「そのヘルパが注入する本番の仕組みを持つ module」）。`tests/` も使うヘルパだけ `#[doc(hidden)] pub mod` で root facade（`seiran_compiler::test_support`）
- test module も use 規約は本体と同じ（`use super::` は直近の親だけ）
- テストコードでは `unwrap` / `expect` / `panic!` 可（属性不要。`expect` メッセージは日本語で期待を書く）。`tests/` から使うヘルパは cfg(test) 外なので本体と同じ扱い。`unwrap_in_result` だけはテスト内でも発火 → `#[expect(clippy::unwrap_in_result, reason = ...)]`
- golden テスト・組版変更の検証・資産取得（初回 `tools/fetch-test-assets.sh`）・golden 再生成は `verify-typesetting` skill

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
