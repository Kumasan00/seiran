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
- バリアントごとに `#[error("...")]`（メッセージ、日本語）と `#[diagnostic(code(<crate>::<category>::<name>), help("..."))]` を付与する。`code` は `<crate>::<category>` を接頭辞にコロン区切りで階層化する（例: `config::validation::field`, `frontend::eval::unknown_command`）
- 外部エラーを巻き取る場合は `#[source] source: ExternalError` フィールドで chain を形成し、`?` 演算子で伝播する。`map_err` でメッセージのコンテキスト（ファイルパス等）を付与する
- **中間の seam エラーを `#[diagnostic_source]` で連鎖させない。** `code` / `help` を持つ Diagnostic を
  `#[diagnostic_source]` に載せると、miette がその変種ぶんの診断ブロックを入れ子で追加描画し、
  利用者から見える出力が 1 段深くなる。資源取得の `config::SourceReadError` のように「呼び出し元が
  パスを含むメッセージを持ち、実質は下位の I/O 失敗」でしかない中間エラーは、`SourceReadError::into_io()`
  で `std::io::Error` へ平坦化してから `#[source]` に載せる（#300）。`#[diagnostic_source]` を使うのは、
  内側のエラー自身が独立した診断として読ませる価値がある場合（`#[label]` / `#[source_code]` を持つ
  パース系エラー等）に限る

## ソース位置付きエラー

- ソーステキストに紐づくエラー（パース・評価系）は `#[label("...")] span: miette::SourceSpan` を持たせる。
- **ソース本文を持つかどうかで扱いが分かれる。** ソース本文（ファイル名・全文）を直接読める場所で
  エラーを構築する場合（例: TOML パーサ呼び出し直後）は、その場で `miette::NamedSource` を保持するラッパー
  enum を返し、変種に `#[source_code] src: NamedSource<String>` と内側のエラーへの
  `#[source] #[diagnostic_source] error: InnerError` を持たせて Diagnostic を伝播してよい。
  一方、ソース本文を持たない下位クレート（呼び出し元が `SourceId → 本文` の対応表（`SourceDb` 等）を
  一元管理している場合。例: `frontend::ParseSourceError` は `crates/seiran::build_pdf::project::SourceDb`
  に対して本文を持たない）は、`#[source_code]` を持たず `source_id`（発行元が単一の識別子。生の `usize`
  や array index を独自に採番しない）だけを運ぶ。この場合、`#[related]` 集約や最終 `Report` 化を行う
  呼び出し側が、`SourceId` から引いた `NamedSource` を添える薄いラッパー型を用意する。このラッパーは
  `#[diagnostic(transparent)]` を使わず（`source_code` も内側へ委譲されてしまうため）、`code` / `severity` /
  `help` / `url` / `labels` / `related` / `diagnostic_source` を内側へ委譲し `source_code` だけを差し替える
  `miette::Diagnostic` を手書きする（例: `seiran::build_pdf::error::AttributedParseError`、issue #299）
- どちらの形でも、ソース ID・array index を独立した場所で 2 回採番しない。1 箇所（`SourceDb::register` 等）
  だけが ID を発行し、他はそれを運ぶだけにする（2 箇所で独立に採番すると、両者の順序が一致するかが
  規約でしか保証されなくなる。#299 以前の `wrap_resolve_error` はこの規約に依存していた —
  実際に誤動作していたわけではないが、`SourceDb` へ統一して依存を除いた）

## 複数エラーの集約

- 複数エラーを 1 度にまとめて報告する場合は `#[related] errors: Vec<...>` を持つ集約バリアント（例: `MultipleValidationErrors`）を作る。`#[related]` の要素は **`Diagnostic` 実装が必須**であり、`miette::Report` は実装しないため、`Report` を直接ベクタに詰めることはできない。クレート固有のエラー型（例: `ParseSourceError`）を返すことでこの問題を回避する

## compiler 内部バグ

- ユーザー入力に起因しない内部不変条件違反が `Result` を返す経路（`.map_err` / `?` の途中）で発覚した場合、
  `panic!` せず専用の小さな struct（例: `seiran::build_pdf::error::CompilerBug`）を作り、通常のユーザー向け
  エラーバリアントには混ぜない
- `#[diagnostic(code(...))]` は `internal_bug` 系のサフィックスにし、`help("...")` はトラブルシュート手順ではなく
  issue 報告を促す文言にする（ユーザー側に誤りがあるわけではないため）
- `unreachable!` との使い分け: 手元に `Result` / `Option` があり、それを使ってエラーを返すほうが panic より
  安全・安価なら `CompilerBug`。手元に `Result` がなく、その状態が構造的に到達不能なら CLAUDE.md 規約 5 の
  `unreachable!` を使う

## シグネチャの原則

- 関数のシグネチャは原則 **クレート固有のエラー型を返す**（例: `Result<Config, ReadConfigError>`, `Result<Vec<DocNode>, ParseSourceError>`）。`miette::Result<T>` は `main` や上位パイプライン関数（`build_pdf`, `layout_engine` 等）でのみ使い、ライブラリ的な公開 API では避ける。`Report` は `Diagnostic` を実装しないので、`#[related]` で集約される可能性のあるエラーは具体型で返すこと
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
  #[diagnostic(code(my_crate::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },

  /// ソース位置付きエラー: #[label] + SourceSpan、NamedSource は呼び出し側で添付
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(code(my_crate::eval::unknown_command), help("コマンド名のスペルを確認してください。"))]
  UnknownCommand {
    name: String,
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 集約バリアント: 検出した違反を 1 度に報告
  #[error("複数のバリデーションエラーが発生しました。")]
  #[diagnostic(code(my_crate::multiple_validation_errors))]
  MultipleValidationErrors {
    #[related]
    errors: Vec<ValidationError>,
  },
}
```

## バリデーション（garde）

設定ファイルの値検証は `garde` の `#[derive(Validate)]` + フィールド属性（`range` / `length` / `ascii` / `dive` / `custom`）で宣言的に記述する。複雑な相互制約は `custom` バリデーターで補い、検出した不正は `*ValidationError::Field { path, message }` に変換して `MultipleValidationErrors { #[related] errors: Vec<...> }` に集約し、すべての違反を 1 度に報告する（config クレートの `ConfigValidationError` / `StyleValidationError` で同パターン）。

例外: `read_references` は集約せず deserialize 時に fail-fast（著者名 / ID 検証）。集約方式に戻さない。
