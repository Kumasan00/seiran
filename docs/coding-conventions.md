# コーディング規約 — 規約の全文・根拠・lint との対応

## この文書の役割

Seiran の Rust コードの書き方の**正典**。`CLAUDE.md`「コーディング規約」節は各規約の本文と編集中に効く
落とし穴だけを持つ要約で、根拠・lint の検出範囲・境界事例・経緯はここに置く。lint の**採用根拠**は
root `Cargo.toml` の `[workspace.lints.*]` に付けたコメント（1 lint = 1 行）が正典で、本書は lint が
要求する**書き方**を持つ（役割分担は「Clippy」節）。

| 文書                              | 持つもの                                                                 |
| --------------------------------- | ------------------------------------------------------------------------ |
| `CLAUDE.md`                       | 規約本文の要約と、編集中に効く落とし穴（ナビゲーション用）               |
| **`docs/coding-conventions.md`**  | **規約の全文・根拠・lint との対応・境界事例（本書）**                    |
| root `Cargo.toml`                 | lint の採用根拠（1 lint = 1 行）と目的別の節見出し                       |
| `clippy.toml` / `rustfmt.toml`    | lint の設定値 / フォーマットの正典                                       |
| `error-handling` skill            | エラー型・診断（code / help / label / related）・集約・garde の設計規約  |
| `verify-typesetting` skill        | golden テスト・PDF バイト比較の使い分けと再生成手順                      |

貫く原理は 1 つ — 言語設計の G1「字面だけで構造が一意」をコードへ適用する。use 規約・値と型の書き方・
`unreachable!` / `expect` の根拠明記はすべてこの適用例で、機械化できるものは lint に落とし、lint が
判定できないもの（enum match の wildcard・`clone` の要否・doc が日本語か・根拠文の質）だけを人が守る。

## 必須ルール

番号は root `Cargo.toml` の lint コメント（「必須ルール 1」等）と `CLAUDE.md` が参照するので**固定**する。

### 1. `return` キーワード必須

関数の返り値には必ず `return` を使用する。末尾式による暗黙の返却は使わない。`implicit_return` が
機械化しており、Clippy の `needless_return` を allow にしているのはこの規約の裏返し。

### 2. フォーマット

正典は `rustfmt.toml`（インデント 2 スペース・最大行幅 120 文字ほか）。手書き時もこれに合わせ、適用は
`cargo +nightly fmt`。nightly toolchain が必須なのは、`rustfmt.toml` で `unstable_features = true`
（`group_imports = "StdExternalCrate"` / `imports_granularity = "Crate"` / `format_macro_bodies` 等）を
有効化しているため。

### 3. use 文

import は「名前を持ち込む」行為であり、**持ち込んだ名前の出自が呼び出し箇所の字面で一意に分かる範囲で
最短パスを使う**（G1 の import への適用）。以下はすべてこの原則から導かれる。

- **起点は `crate::` に統一**: `use` は `crate::` 起点で書き、`super::` / `self::` 起点は使わない（同じ型に
  2 本のパスを作らないため。root ファサードと同じ理由）。例外は 2 つだけ —
  (a) 同じファイルが `mod` 宣言している子 module から取り込む相対 use（`use input::CompilationInputs;`。
  `mod input;` が同じ画面にあるので出自が字面で見え、隣の `pub use warnings::Warnings;` と同じ形になる）、
  (b) `#[cfg(test)]` の module（`mod tests` / `mod test_support`）が**直近の親**の被テスト項目を取り込む
  `use super::*` / `use super::Item`。(b) は親 1 段までで、`super::super::` で祖父母以上へ遡る形は
  使わない（何段上かを数えないと出自が分からず、原則に反する）。
- **`crate::` を本体コードへ直書きしない**: 型・トレイトは import して裸の名前で書き、関数は import した
  module 経由で `module::fn(...)` と呼ぶ。`clippy::absolute_paths` は 4 セグメント以上しか検出しない
  （`clippy.toml` の `absolute-paths-max-segments = 3`。末尾の enum variant / 関連関数を数えないので
  `crate::source::SourceId::new` は素通りし、`project::config::load` と `style::load` を書き分ける
  イディオムは温存される）ので、規約のほうが lint より厳しい。短いパス側は rustc の
  `unused_qualifications` が押さえていて、スコープに入っている名前を `std::sync::Arc::new` のように
  再修飾すると落ちる。どちらの lint も `#[cfg(test)]` の中で発火するので、テストにも同じ規約が効く。
  doc コメント内の intra-doc link（``[`crate::Foo`]``）は絶対パスが正しいので対象外。
- `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`。
- 型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、
  呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの
  re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい。
- `#[cfg(test)]` の項目だけが使う import は `#[cfg(test)] use ...;` と書いて本体ビルドから外す（本体の
  use ツリーへ混ぜると非テストビルドで unused になる）。

### 4. ドキュメントコメント

すべてのモジュール・型（struct / enum / trait）・関数に**日本語**で記述する。`missing_docs_in_private_items`
と rustc の `missing_docs` の 2 枚で全項目の**有無だけ**が検査される。日本語で書かれているかは検査されない
ので人が見る。

### 5. `unreachable!` は積極的に使う

まず型設計で到達不能な状態自体を表現不能にできないか検討し、それでも残る「絶対に到達しない」分岐は
`_ => {}` / `Default::default()` / 黙って `Ok` を返す等でごまかさず `unreachable!` で落とす（不変条件の
破れを最寄りで顕在化させる）。ただし入力（ソース・設定ファイル）由来で到達しうる状態は panic ではなく
miette 診断エラーにする（`error-handling` skill）。本体コードでは「なぜ到達しないか」＝上流のどの検証が
保証しているかをメッセージに書く（例: `unreachable!("許可リスト外は strict_command_calls がエラーにする")`。
テストの let-else 分解など自明な箇所は省略可）。

### 6. panic は根拠を書いてから落とす

- **`.unwrap()` → `.expect("...")`**: 本体コードで `.unwrap()` を使わず `.expect("...")` に移し、メッセージには
  条件の言い換えではなく「なぜ落ちないか」＝上流のどの検証・型設計が保証するかを日本語で書く
  （`unwrap_used`。`unreachable!` / assert メッセージと同じ書き方）。同じ根拠が何箇所にも並ぶなら、根拠ごと
  ヘルパ関数へ畳んで 1 箇所にする（`frontend::syntax::parser` の `take_peeked` / `peeked`）。
- **assert にもメッセージ**: `assert!` / `debug_assert!` 系にもメッセージが必須（`missing_assert_message`。
  テストコードでは発火しないので素の `assert_eq!` でよい）。
- **`-> Result` の中では panic しない**（`panic_in_result_fn`）: 入力由来で到達しうる状態は miette 診断
  エラーにして `Err` で返す。この lint が見るのは `panic!` / `todo!` / `unimplemented!` と `assert!` 系で、
  **`unreachable!` と `debug_assert!` は発火しない**ので必須ルール 5 と衝突しない。
- **裸の `panic!` を書かない**（`panic`）: `-> Result` の外でも本体コードに裸の `panic!` は書かない — 到達
  しないなら `unreachable!`、入力由来なら miette 診断で、どちらでもない `panic!` は残らないため。テストは
  `allow-panic-in-tests` で対象外だが、`tests/` から使うヘルパは `#[cfg(test)]` の外なので発火する
  （`unwrap_used` と同じ）。`#[cfg(test)]` の中でも発火するため、`?` のために `-> Result` を返すテスト関数
  では `assert!` ではなく `unwrap` / `expect` で落とす。
- **`unwrap_in_result`**: その `unwrap` / `expect` の側はこの lint が見る — `Result` / `Option` を返す関数の
  中で**返り値と同じ型ファミリ**の `.unwrap()` / `.expect()` を書いたら、
  `#[expect(clippy::unwrap_in_result, reason = ...)]` を関数に付けて例外だと宣言する（この lint は
  `allow-unwrap-in-tests` を見ないので `#[cfg(test)]` の中でも発火し、上の「`-> Result` を返すテスト関数は
  `unwrap` / `expect` で落とす」もこの属性とセットになる。関数単位なので中の複数箇所を 1 枚で覆える）。
  **この lint は網羅ではない** — 型ファミリが食い違う組（`-> Result` の中の `Option::expect`、`-> Option`
  の中の `Result::expect`）とクロージャの本体は発火しないので、属性が無いことは「根拠を検討済み」を
  意味しない（`missing_docs*` が doc の有無だけを見て日本語かは見ないのと同じ。根拠を書く責任は lint では
  なく本ルールの側にある）。
- **`reason` に書けるのは 2 語義だけ**: 「panic が必要」か「panic し得ない」のどちらかで、どちらも書けない
  なら属性ではなく診断エラー化かリファクタで直す。`.expect()` 自体は「根拠を書いた上での逃げ道」として
  残してあり、`expect_used` は有効化していない。

### 7. unsafe は 1 操作 1 ブロック 1 SAFETY

`unsafe {}` 1 つに unsafe 操作は 1 つだけ置き（`multiple_unsafe_ops_per_block`）、その直上の行に
`// SAFETY:` を書く（`undocumented_unsafe_blocks`。間に別の文を挟むと検出されない）。「どの操作のどの前提が
根拠か」を 1 対 1 で対応させるためで、逆に unsafe を含まない箇所へ SAFETY コメント・`# Safety` doc を
書かない（`unnecessary_safety_comment` / `unnecessary_safety_doc`。「ここに検証済みの unsafe がある」という
誤読を招く死んだ注釈になる）。周辺コードの前提を説明したいときは `// NOTE:` 等の別の語で書く。
`unsafe fn` の本体でも `unsafe {}` を書く（`unsafe_op_in_unsafe_fn`）。

## モジュール構成

- **`mod.rs` を使わない**（`mod_module_files`）: サブモジュールを持つモジュールは、2018 エディション以降の
  スタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、
  子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs    ← 子モジュール
  ```

  例外: 統合テスト（`tests/`）の共通ヘルパは慣例どおり `tests/common/mod.rs` に置く（`common.rs` だと
  テストファイルとして扱われるため。この lint の検査対象外なのでそのまま置ける）。

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート
  root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に
  `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` / `pub(crate) mod` はモジュール名が
  名前空間として意味を持つ場合のみ（例: `project::config` は入口を `project::config::load` と読ませて
  `style::load` と区別する。crate root 直下の非公開 module は crate 全体から到達できるため、garde カスタム
  バリデータを持つ `length` に `pub(crate)` は不要）。同名の型を 2 つ作って module 公開で回避しない —
  名前側を変えて衝突自体を無くす（例: `ConfigValidationError` / `StyleValidationError`）。root facade へ
  載せるのは実際に名指しされる名前だけで、内部フィールド型としてしか現れない名前は再エクスポートしない
  （この 2 方向は rustc の `unreachable_pub` と `unnameable_types` が機械化している — 外から到達しない
  `pub` は狭め、公開シグネチャに現れるのに facade から名指しできない型は facade へ出すか宣言を狭める）。
  利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と
  書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を
  確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割
  しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した
  塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を
  緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで
  `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを
  既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開した
  ほうが利用側にとって分かりやすい場合は、API を変更してよい。

## 値と型の書き方

字面から意味が読めることを優先する（G1 のコードへの適用）。末尾の enum match の項を除き lint が機械化して
いる。

- `Rc` / `Arc` の複製は `Rc::clone(&x)` / `Arc::clone(&x)` と関連関数形で書く（`clone_on_ref_ptr`）。
  `x.clone()` は「参照カウントを増やしただけ」なのか「中身を deep copy した」のかが字面で区別できず、
  型宣言まで遡らないと読めない。`rc::Weak` / `sync::Weak` も対象。
- そのまま move すれば済む値を `clone()` しない（`redundant_clone`）。複製先も元もその後読まれないなら、
  複製は無駄なだけでなく「ここは所有権を分ける必要がある」という誤った合図を残す。nursery lint だが
  Known problems は **false-negative**（解析が保守的）なので、発火したら真陽性として直す。裏返すと
  **この lint が通っても「無駄な `clone` が無い」証明にはならない**ので、`clone` を書く判断そのものは人が
  やる。
- `Eq` を導出できる型に `PartialEq` だけを derive しない（`derive_partial_eq_without_eq`）。`PartialEq`
  止まりは「等価が部分的（`a == a` が成り立たない値がある）」という主張になるので、実際には全等価な型に
  付けると利用側が「NaN のような例外値があるのか」を判断できない。`PartialEq` 止まりで残るのは f32 / f64
  を持つ型だけ（そこでは lint が発火しない）。`Length` は sp（1/65536pt の整数）なので float を経由しない
  限りこの方針に乗る。derive の並びは `Debug, Clone, Copy, PartialEq, Eq, Hash` の順。
- 公開型には `Debug` を実装する（`missing_debug_implementations`）。derive で生バイト列が出力に載る型
  （読込キャッシュ・フォント・画像）は手書きにして件数・長さだけを出す（`publication` /
  `project::filesystem` の `Debug`）。
- 借用を持つ型は `Foo<'_>` と書く（`elided_lifetimes_in_paths`）。`Foo` と書けると借用の有無が字面から消え、
  宣言まで遡らないと読めない。
- 型推論で足りる `as` は書かない（`trivial_casts`）。trait object から auto trait（`Send` / `Sync`）を落とす
  変換のように**外すとコンパイルが通らない**キャストで発火することがあり、そこだけ `#[expect]` + 理由で
  残す（`compiler::compile_failure`）。
- 数値リテラルの型サフィックスは `1u32` 形（`separated_literal_suffix`）。`1_u32` 形と混在させない。
- 識別子は ASCII で書く（`non_ascii_idents`）。テスト名も同じで、日本語は doc コメント・診断メッセージ・
  assert の文言に置き、名前には持ち込まない。

### enum match の wildcard 判定（lint ではなく人が守る）

enum を match するときは「入力 enum に variant を追加したら、この処理の判断を必ず見直すべきか」で wildcard
の可否を決める。

- **Yes なら wildcard を使わず全 variant を明示する**（同じ結果になる variant は `|` でまとめる）— enum 間・
  enum から値への意味的な対応表、parser / evaluator の dispatch、状態遷移、段間語彙の完全走査、新しい
  variant がアルゴリズムへ参加するかを必ず判断すべき分類がこれにあたり、明示しておくと variant 追加が
  見直すべき箇所でコンパイルエラーになる。
- **No なら wildcard を維持する** — ある variant だけを取り出す抽出・検索、明示された部分集合だけを判定する
  述語、許可リスト以外を同じ診断にする既定エラー、非対象値を変更せず通す pass-through、enum 自身の共通
  操作へ委譲する分岐では「新しい variant も既定へ入る」ことが処理の意味なので、網羅化すると意味が壊れる。

`clippy::wildcard_enum_match_arm` はこの Yes / No を区別できず両方を等しく報告するので有効化しない
（#402 / #426）。ガード付き arm と網羅性判定の関係に注意する（`Penalty { .. } if cond` のようなガード付き
arm は網羅性判定に参加しないので、同じ variant を wildcard 側の列挙にも書く。逆に
`Glue { breakable: true, .. }` のようなリテラルパターンは参加する）。

## エラーハンドリング・バリデーション

正典は `error-handling` skill — 新しいエラー型の定義・バリアント追加・診断（code / help / label /
related）の設計・ソース位置付与・garde バリデーション追加の際は必ず参照する。常時効く原則は以下。

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語）。
  `miette::Result<T>` は CLI 入口だけで使い、内部パイプラインは具体的なエラー型を保つ。
- `compile` の失敗型は不透明型 `CompileFailure`（先頭が主診断・空で構築不能）。ユーザーが最初に読む
  メッセージは常に修正可能な leaf diagnostic にする。
- 診断 `code` の第 1 階層は「段」の固定列挙（`project` / `style` / `frontend` / `semantics` / `typeset` /
  `compiler` / `pdf` / `cli` の 8 つ）、第 2 階層以降は著者が選ぶ意味的カテゴリ（module パスではない）。
- 設定値検証は `garde` で宣言的に書き、違反は `Failures<E>` に集めて 1 度に報告する。
- 集約するかは「失敗後も独立な検査を安全かつ決定的に続けられるか」で決め（#376）、表示順は入力の論理順
  （`HashMap` の反復順や rayon の完了順に依存させない）。
- 集約自身に診断 `code` を付けない（`Failures<E>` は `Diagnostic` を実装しないことで型保証）。
- 外部資源の read error は低水準 cause として、所有段が作る leaf diagnostic の `#[source]` に入れる（#377）。
- warning は error と公開型を共用せず（`Warnings`）、同じ問題を診断と tracing の両方で出さない（#377）。
- `map_err(|_| ...)` で元のエラーを黙って捨てない（`map_err_ignore`）。低水準の cause は leaf diagnostic の
  `#[source]` に入れて包み、`ParseFloatError` のように値を持たず本当に捨ててよい場合は `let ... else` /
  `ok_or_else` で書く（捨てていることが字面に出る形にする）。
- `Result` を `.ok()` で捨てない（`unused_result_ok`）。無視してよい失敗なら `let _ = f();` と書いて捨てて
  いること自体を字面に出す。値が要る場合は `.ok()` のまま `?` / `unwrap_or` へ繋げてよく、この lint は
  結果を使わない場合だけ発火する。
- 診断 enum が大きいこと（`result_large_err`）は workspace の allow で畳んである。miette の位置情報を運ぶ
  以上どの診断も大きくなり、これは「thiserror + miette の富んだ診断を採る」という**種類**の判断 1 個の
  帰結だから（P10 の判定テストと同型）。逆に `cast_*` 系・`too_many_arguments`・`ref_option` は箇所ごとに
  根拠が違うので per-site の `#[expect]` のまま。
- ライブラリ 2 crate では `println!` / `eprintln!` を使わない（`print_stdout` / `print_stderr`）。人へ
  伝えたいことは診断（miette）か tracing に載せる。表示はユーザーへ届ける成果物なので、CLI（`seiran`）の
  crate root だけ `#![expect]` で開けてある。
- 決定性が要る場所で `HashMap` / `HashSet` を `for` 反復しない（`iter_over_hash_type`）。`RandomState` の
  反復順はプロセスごとに変わるので、出力順・採番順・配列添字にすると成果物が非決定になる。`BTreeMap` を
  使うか、キーで `sort` した `Vec` に落としてから反復する。この lint が見るのは `for` だけなので
  `values().sum()` のような順序非依存の集計は発火しない — 逃げ道として使うのではなく、「順序に依存して
  いない」と言い切れるときだけそう書く。

## Clippy

### 役割分担

**lint の採用根拠の正典は root `Cargo.toml`** — `[workspace.lints.clippy]` / `[workspace.lints.rust]`（各
クレートは `lints.workspace = true` で両テーブルを継承）に 1 lint = 1 行の根拠コメントが付いていて、
節見出しが下の目的に対応する。lint 側の設定値は root `clippy.toml`（`absolute-paths-max-segments = 3` /
`allow-unwrap-in-tests = true` / `allow-panic-in-tests = true`）。lint の増減と根拠の更新が同じ diff に
閉じるように、個々の lint の理由は `Cargo.toml` のコメントに書き、個々の lint が要求する**書き方**は
本書の規約各節に置く。`CLAUDE.md` の Clippy 節は運用（CI の形・抑制の作法）だけを持つ。

### 有効化の目的（節見出し）

`Cargo.toml` の節見出しは **lint を有効化する目的**で、目的に載らない lint は採らない。

| 目的                      | 何を守るか                                                                  | 例                                                                  |
| ------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| 規約の機械化              | 名指しの規約（必須ルール / use 規約 / モジュール構成 / テスト）を機械に守らせる | `implicit_return` / `missing_docs*` / `mod_module_files` / SAFETY 族 |
| 字面に意味（G1 の読み側） | 字面から意味が一意に取れる                                                  | `clone_on_ref_ptr` / `map_err_ignore` / `elided_lifetimes_in_paths` |
| 表記の固定（G1 の書き側） | 同じ意味を 2 通りに書かない                                                 | `separated_literal_suffix` / `pub_without_shorthand`                |
| 決定性                    | 成果物のバイト再現性を壊すものを拒む                                        | `iter_over_hash_type` / `float_cmp_const`                           |
| crate の責務境界          | ライブラリ 2 crate はプロセス境界の効果（終了・表示）を持たない             | `exit` / `print_stdout`                                             |
| 誤りの検出                | 取り違え・バグ源になる形を拒む                                              | `rc_mutex` / `unit_bindings`                                        |
| 残骸を残さない            | 死んだコード・デバッグ残りを残さない                                        | `dbg_macro` / `unused_macro_rules`                                  |
| nursery                   | group が未安定なので目的別に散らさず 1 節に集める（目的は各行の根拠コメント） | `redundant_clone` / `fallible_impl_from`                            |

1 つの lint が 2 つの目的に載ることはある（`non_ascii_idents` は「値と型の書き方」の規約表にもある）。置き場の
規則は 1 つ — **根拠コメントが名指しの規約を引くなら「規約の機械化」、それ以外はコメント自身の目的の節**。
「本書に書き方の記述があるか」は規則にできない（採用 lint はすべて本書に書き方を持つので、全節が規約へ流れ込む）。

### 採用条件

目的は「なぜ有効化するか」で、これとは別に「どう選んだか」の条件がある。選定の履歴であって lint を守る理由
ではないので、`Cargo.toml` の見出しには出さない。

- 発火 0 件で規約と整合する lint は無料の再発防止として採る。0 件は lint 単位で実測する（手順は「運用」節）
- 2 通り書ける形は現行スタイル側（0 件の側）に固定する
- nursery は Known problems を理解した lint だけを 1 件ずつ、根拠コメント付きで採る

`clippy::all` が deny、`pedantic` が warn で、group を個別 lint が上書きする（group は priority -1、個別は
既定の 0）。

lint が**発火＝提案に従う**とは限らない — `suboptimal_flops` / `imprecise_flops` は「float の合成演算を
ここで書いた」ことの通知として有効化してあり、採否は箇所ごとに決める（項を累積して精度が効くなら
`mul_add` / `ln_1p` を採る・値が一度動くので golden 再生成込み／1 項で定数畳み込みや可読性が勝つなら積へ
名前を付けて式から乗算を外す／どちらでもないなら `#[expect]` + 根拠）。`mul_add` は IEEE の単一丸めで
`a * b + c` と同じく決定的なので、決定性の懸念は移行時の値の変化に限られる。

採らないと決めた lint の理由は #402 に記録がある（恒久不採用と、条件が変われば再検討するものを分けてある）。

### 運用

- CI と pre-commit フックは `cargo clippy --all-targets --all-features -- -D warnings` で走る。warn レベルの
  指摘もそこでビルド失敗になるため、素の `cargo clippy` ではなくこの形で確認する。
- **抑制は `#[expect(...)]` + `reason = "..."` だけ**（`allow_attributes` / `allow_attributes_without_reason`）。
  `allow` は抑制対象が消えても黙って残るが、`expect` なら rustc の `unfulfilled_lint_expectations` が
  「効いていない抑制」を落とすので、**有効化されていない lint を抑制しない**・**抑制対象が消えたら属性も
  消す**が機械的に守られる。`reason` には「なぜ許してよいか」＝上流のどの保証・どの設計判断が根拠かを
  書き、lint 名の言い換え（`reason = "truncation を許す"`）は書かない。根拠が言えない属性は書き足さずに
  直す（`dead_code` は本当に未使用なら削除する）。
- 本体ビルドでだけ発火する lint（`#[cfg(test)] mod tests` だけが使う項目に対する `dead_code` /
  `unused_imports`）は `#[cfg_attr(not(test), expect(...))]` と書く。素の `#[expect]` はテストビルドで充足
  せず `unfulfilled_lint_expectations` に落ちるため、`allow` へ戻すのではなく「本体ビルドでだけ抑制する」
  ことを字面に出す。
- **新しい lint の 0 件は lint 単位で実測する**。
  `cargo clippy --all-targets --all-features --message-format=json -- -W clippy::<name>` を回し、診断コード
  （`message.code.code`）で数える。短縮フォーマットの grep は出力に lint 名を含まないため全件 0 に見える
  （この偽陰性で 14 件が隠れた実例がある）。

## テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei`
  ファイル群）、フォント: リポジトリ直下の `fonts/`。
- AAA パターンで記述し、Arrange / Act / Assert が実際に複数行へ分かれるテストだけ `// Arrange` /
  `// Act` / `// Assert` で区切る（fixture 組み立て → `layout` → ページ検証のような組版テストがこれに
  あたる）。1〜3 行で 3 段が分離しないテストには付けない — `// Arrange / Act` の合体表記が要るなら、その
  テストに区切りは不要という合図。テスト名に `test_` 接頭辞は付けない（`redundant_test_prefix`）— 何を
  検証するかだけを書く。
- **共有ヘルパ**: 3 つ以上の test module が同じヘルパを必要としたら、各 module へ複製せず `#[cfg(test)]`
  で閉じた `test_support` module に切り出して 1 箇所に集める（`frontend::evaluator::test_support` /
  `typeset::lowering::test_support`）。切り出し先は「そのヘルパが注入する本番の仕組みを持つ module」で、
  呼び出し側は `test_support::parse(...)` のように module 経由で呼ぶ。crate 外の統合テスト（`tests/`）も
  使うヘルパだけは例外で、`#[cfg(test)]` では閉じられないので `#[doc(hidden)] pub mod` として root facade
  に載せる（`project::config::test_support` → `seiran_compiler::test_support`）。
- test module も本体と同じ use 規約に従う（必須ルール 3）。親の被テスト項目を `use super::*` /
  `use super::Item` で取り込むのは許容だが、それ以外は `crate::` 起点で import する。
- テストコードでは `unwrap` / `expect` / `panic!` を許容する（`unwrap_used` / `panic` は `clippy.toml` の
  `allow-unwrap-in-tests` / `allow-panic-in-tests` で対象外、`expect_used` は無効なので、いずれも属性は
  付けない）。`expect` のメッセージは日本語で期待を書く（例: `"一時ファイルを作成できるはず"`）。ただし
  `tests/` から使うヘルパは cfg(test) の外なので `unwrap_used` / `panic` が効く（本体と同じく `.expect()` +
  根拠。エラー内容の整形が必要で `.expect()` の Debug では足りない箇所だけ
  `#[expect(clippy::panic, reason = ...)]`）。`unwrap_in_result` だけは例外で `allow-*-in-tests` 相当の設定を
  持たず `#[cfg(test)]` の中でも発火するので、`Result` / `Option` を返すテストヘルパで返り値と同じ型
  ファミリを `unwrap` したら `#[expect(clippy::unwrap_in_result, reason = ...)]` を付ける
  （`frontend::evaluator::environment::table` の `eval_table` / `project::config` の
  `run_validate_with_serif_extra`）。
- **golden テスト・組版変更の検証**: レイアウトダンプ golden（`crates/seiran-compiler/src/compiler/golden.rs`）と
  PDF バイト比較の使い分け、前提資産の取得（初回は `tools/fetch-test-assets.sh` を 1 度実行）、golden の
  再生成、新機能へのテスト追加は `verify-typesetting` skill を参照する。
