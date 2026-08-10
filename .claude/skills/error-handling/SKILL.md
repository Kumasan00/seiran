---
name: error-handling
description: >-
  Seiran のエラーハンドリング・バリデーション規約。新しいエラー型を定義する時、
  既存エラー enum にバリアントを足す時、miette 診断（code / help / label / related）を
  設計する時、ソース位置付きエラーや複数エラーの集約を返す時、garde で設定値の
  バリデーションを書く時に必ず参照する。
---

# エラーハンドリング

## エラー型の定義

- 各クレートの `lib.rs`（または該当モジュール）に `thiserror::Error` + `miette::Diagnostic` 派生のエラー列挙型を定義する。`#[derive(Debug, Error, Diagnostic)]` を常に併用する
- バリアントごとに `#[error("...")]`（メッセージ、日本語）と `#[diagnostic(code(...), help("..."))]` を付与する。`code` の付け方は次節の規約に従う
- 外部エラーを巻き取る場合は `#[source] source: ExternalError` フィールドで chain を形成し、`?` 演算子で伝播する。`map_err` でメッセージのコンテキスト（ファイルパス等）を付与する
- **中間の seam エラーを `#[diagnostic_source]` で連鎖させない。** `code` / `help` を持つ Diagnostic を
  `#[diagnostic_source]` に載せると、miette がその変種ぶんの診断ブロックを入れ子で追加描画し、
  利用者から見える出力が 1 段深くなる。資源取得の `project::SourceReadError` のように「呼び出し元が
  パスを含むメッセージを持ち、実質は下位の I/O 失敗」でしかない中間エラーは、`SourceReadError::into_io()`
  で `std::io::Error` へ平坦化してから `#[source]` に載せる（#300）。`#[diagnostic_source]` を使うのは、
  内側のエラー自身が独立した診断として読ませる価値がある場合（`#[label]` / `#[source_code]` を持つ
  パース系エラー等）に限る

## 診断 `code` の規約

**第 1 階層は「段」を表す固定列挙で、これ以外の語を第 1 階層に置かない。**

| 段 | 対象 |
| --- | --- |
| `project` | 外部資源取得・config.toml・フォント資源の宣言 |
| `style` | style.toml |
| `frontend` | 字句・構文解析・評価 |
| `semantics` | 意味解析・引用・書誌 |
| `typeset` | 組版（フォント解析・画像・版面の幾何を含む） |
| `compiler` | compile facade（段の集約・横断） |
| `pdf` | `seiran-pdf`（描画） |
| `cli` | `seiran`（PDF 出力の書き込み・サブコマンド） |

**第 2 階層以降は規定しない** — 著者が選ぶ意味的カテゴリで、module パスと一致していなくてよい
（`frontend::eval::unknown_command` の `eval`、`project::config::validation::field` の `validation`、
`frontend::parse_source::syntax` の `parse_source` はいずれも module 名ではない）。

crate 名（`seiran_compiler::`）を第 1 階層に置かない理由: 全 code の約 9 割に付いて情報量がゼロ
（ユーザから見ればバイナリは 1 つ）であり、かつ第 2 階層以降が野放しになるので
「存在しない module 名を名乗る code」（#349 の `resolve::` / #356）を構造的には防げないため。
逆に第 1 階層を段に閉じれば、module を移設しても code が嘘をつくのは段を跨いだときだけになる。

段を跨ぐ wrapper 型は、自分の所有 module ではなく**エラーの出自の段**を名乗る
（`compiler` が所有する `SourceDiagnostic<SemanticError>` が内側の `semantics::*` code を委譲するのが
その形）。

**`code` は leaf diagnostic にだけ付ける**（#375）。段名や集約の都合しか表さない wrapper に
独自の message / `code` / help を与えてユーザー表示へ出さない — ユーザーが最初に読むメッセージは常に
「修正できる leaf」であるべきで、「複数のエラーが発生しました」「◯◯段に失敗しました」を先頭に置かない。
`?` で運ぶための union（例: `frontend::ParseSourceError`）は `#[error(transparent)]` +
`#[diagnostic(transparent)]`、表示単位ですらない制御フロー型（例: `semantics::AnalyzeError`）は
`Diagnostic` を実装しない、が既定形。

`code` の変更はユーザから見える診断出力の変更なので、`tests/golden_diagnostics/` の再生成
（`UPDATE_GOLDEN=1 cargo test -p seiran-compiler`）と差分確認をセットで行う。

## ソース位置付きエラー

- ソーステキストに紐づくエラー（パース・評価系）は `#[label("...")] span: miette::SourceSpan` を持たせる。
- **ソース本文を持つかどうかで扱いが分かれる。** ソース本文（ファイル名・全文）を直接読める場所で
  エラーを構築する場合（例: TOML パーサ呼び出し直後）は、その場で `miette::NamedSource` を保持するラッパー
  enum を返し、変種に `#[source_code] src: NamedSource<String>` と内側のエラーへの
  `#[source] #[diagnostic_source] error: InnerError` を持たせて Diagnostic を伝播してよい。
  一方、ソース本文を持たない下位 module（本文は `project::SourceSet` が一元管理する。例:
  `frontend::ParseSourceError` の内側の `ParserError` / `EvalError`、`semantics::SemanticError`）は、
  `#[source_code]` を持たず span と `SourceId`（発行元が単一の識別子。生の `usize` や array index を
  独自に採番しない）だけを運ぶ。本文の添付は **compiler seam の汎用 adapter
  `compiler::source_diagnostic::SourceDiagnostic<E>` 1 つ**が行う（段ごとの attribution wrapper を
  新しく作らない。#299 / #375）。この adapter は `#[diagnostic(transparent)]` を使わず
  （`source_code` も内側へ委譲されてしまうため）、`code` / `severity` / `help` / `url` / `labels` /
  `related` / `diagnostic_source` を内側へ委譲し `source_code` だけを補う `miette::Diagnostic` を
  手書きしている
- どちらの形でも、ソース ID・array index を独立した場所で 2 回採番しない。1 箇所
  （`project::SourceSet::register`）だけが ID を発行し、他はそれを運ぶだけにする（2 箇所で独立に
  採番すると、両者の順序が一致するかが規約でしか保証されなくなる。#299）
- **1 診断が持てる `source_code` は 1 つ**。複数ソースに跨って見つかる問題（例: 未定義引用キー）は、
  検出した段自身がソースごとの leaf 診断へ分割する（`semantics::SemanticFailures`）。分割を
  compiler 側に置くと、診断文・`code`・help の複製がそちらに生まれる（#375）

## 複数エラーの集約

**集約するかどうかは種類ではなく「失敗後も独立な検査を安全かつ決定的に続けられるか」で決める**（#376）。

- **集約する**: config.toml / style.toml の独立フィールド違反、設定に列挙された複数パスの読込失敗、
  source ごとの parse / eval error、文書全体の重複ラベル・未解決参照・未知引用キー、
  `FontType::ALL` の各フォント検証、各画像の読込・デコード失敗
- **早期 return する**: config.toml 自体を読めない、TOML を parse できない、style path を確定できない、
  HIR を作れない、`SemanticDocument` を作れない、backend が継続不能な失敗を返した。
  **段の間**（config → style → 横断検証、parse → metrics → validate）は後段の入力を構築できないので
  跨いで集約しない — 集約するのは段の中だけ

段の内部で集めた複数の違反は **`crate::failures::Failures<E>`**（crate root の非公開 leaf module）で運ぶ。

- `Failures<E>` は **`miette::Diagnostic` を実装しない**。これが「aggregate 自身に新しい診断 `code` を
  付けない」の型による実装で、集約はそれ自体では描画されず `compiler` seam の
  `CompileFailure::from(failures)` で平坦化されて初めてユーザー表示になる
- 空では構築できない（`single` / `from_vec`（空なら `None`）だけが構築経路で、`Default` は無い）
- **`#[related] errors: Vec<...>` を持つ集約バリアントを新しく作らない。** `MultipleValidationErrors` /
  `MultipleFontValidationErrors` のような「複数のエラーがあります」「◯◯の検証に失敗しました」は
  #376 ですべて削除した。`#[related]` を使ってよいのは、**同じ 1 つの問題を複数箇所で示す**場合
  （`SemanticError::UnknownCitationKeys` が 1 ソース内の複数 `\cite` を `#[label(collection)]` で並べる形）
  に限る。異なる修正を要求する違反は集合の別要素にする
- **表示順は入力の論理順**であり、`HashMap` の反復順や並列処理の完了順に依存させない
  （source は `config.sources` の宣言順、フォントは `FontType::ALL` 順、画像は正規化済み `ProjectPath` の
  昇順、意味解析は `NodeId` 由来の文書順）。rayon を使う箇所は `collect::<Result<Vec<_>, E>>()`
  （複数エラー時にどれが返るか非決定）ではなく `collect::<Vec<Result<_, E>>>()` +
  `failures::collect_in_input_order` を通し、入力順の slot に戻してから集約する
- **段を跨いで `compile` の外へ出す集合は `compiler::CompileFailure`**（#375）。先頭が主診断・残りが
  関連診断で、集約そのものを表す診断（「複数のエラーが発生しました」）を先頭へ足さない。中身は
  `Box<dyn Diagnostic + Send + Sync>` の列で `Vec<miette::Report>` にはしない（`Report` は `Diagnostic`
  を実装しないので `related` へ載せられない）。1 件のときは `into_report` が
  `miette::Report::new_boxed` で leaf をそのまま返し、包む前後で表示が完全に一致する
- leaf に「どの資源の違反か」を添える必要があるときは、集約 wrapper ではなく
  **帰属 adapter** を作る（`typeset::font::validate_font::FontValidationFailure` が
  `code` / `help` / `labels` を内側の kind へ委譲し、メッセージにだけ config.toml のキーを前置する形。
  `SourceDiagnostic<E>` と同じ形で、描画は leaf 1 件ぶん・入れ子の診断ブロックを作らない）

## compiler 内部バグ

- ユーザー入力に起因しない内部不変条件違反が `Result` を返す経路（`.map_err` / `?` の途中）で発覚した場合、
  `panic!` せず専用の小さな struct（例: `seiran_compiler::typeset::error::TypesetBug`）を作り、通常のユーザー向け
  エラーバリアントには混ぜない
- `#[diagnostic(code(...))]` は `internal_bug` 系のサフィックスにし、`help("...")` はトラブルシュート手順ではなく
  issue 報告を促す文言にする（ユーザー側に誤りがあるわけではないため）
- `unreachable!` との使い分け: 手元に `Result` / `Option` があり、それを使ってエラーを返すほうが panic より
  安全・安価なら `TypesetBug` 相当の内部バグ型。手元に `Result` がなく、その状態が構造的に到達不能なら CLAUDE.md 規約 5 の
  `unreachable!` を使う

## シグネチャの原則

- 関数のシグネチャは **常に具体的なエラー型を返す**（例: `Result<Config, ReadConfigError>`,
  `Result<HirSource, ParseSourceError>`, `Result<Compilation, CompileFailure>`）。**production の内部
  pipeline で `miette::Result<T>` を使わない**（#375）— `miette::Report` への型消去は CLI 入口
  （`main` / サブコマンド）でだけ行い、そこまでは段の error 型を保つ。`Report` は `Diagnostic` を
  実装しないので、早期に型消去すると `#[related]` にも `CompileFailure` にも載せられなくなる
- 外部クレートの `Result<T, E>` を `miette::Result<T>` に持ち上げる際は `miette::IntoDiagnostic` の `.into_diagnostic()?` を使用する
- `main` は `miette::Result<()>` を返す（`Box<dyn std::error::Error>` は使わない）。`miette` の `fancy` feature により色付き診断が出力される

## パターン例

```rust
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum MyError {
  /// I/O 失敗: 外部エラーを #[source] で連鎖
  #[error("ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(project::config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },

  /// ソース位置付きエラー: #[label] + SourceSpan、NamedSource は呼び出し側で添付
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(code(frontend::eval::unknown_command), help("コマンド名のスペルを確認してください。"))]
  UnknownCommand {
    name: String,
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 値検証の違反 1 件: 集約バリアントを作らず、内側の leaf をそのまま透過させる
  /// （複数の違反は `Failures<MyError>` の別要素として並ぶ）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] ValidationError),
}
```

## バリデーション（garde）

設定ファイルの値検証は `garde` の `#[derive(Validate)]` + フィールド属性（`range` / `length` / `ascii` / `dive` / `custom`）で宣言的に記述する。複雑な相互制約は `custom` バリデーターで補い、検出した不正は `*ValidationError::Field { path, message }` に変換し、`Failures<Read*Error>`（各違反は `Read*Error::Validation` で透過）としてすべての違反を 1 度に報告する（`project::config` の `ConfigValidationError` / `style` の `StyleValidationError` で同パターン）。集約自身の診断（旧 `MultipleValidationErrors`）は作らない — ユーザーが最初に読むのは「どのフィールドをどう直すか」であるべきだから（#376）。

例外: `read_references` は集約せず deserialize 時に fail-fast（著者名 / ID 検証）。集約方式に戻さない。
