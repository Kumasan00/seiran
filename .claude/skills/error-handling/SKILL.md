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

## ソース位置付きエラー

- ソーステキストに紐づくエラー（パース・評価系）は `#[label("...")] span: miette::SourceSpan` を持たせる。エントリポイント（例: `frontend::parse_source`）では `miette::NamedSource` を保持するラッパー enum（例: `ParseSourceError`）を返し、変種に `#[source_code] src: NamedSource<String>` と内側のエラーへの `#[source] #[diagnostic_source] error: InnerError` を持たせて Diagnostic を伝播する。これにより `#[related]` 集約時もソースコード付きの label がレンダリングされる

## 複数エラーの集約

- 複数エラーを 1 度にまとめて報告する場合は `#[related] errors: Vec<...>` を持つ集約バリアント（例: `MultipleValidationErrors`）を作る。`#[related]` の要素は **`Diagnostic` 実装が必須**であり、`miette::Report` は実装しないため、`Report` を直接ベクタに詰めることはできない。クレート固有のエラー型（例: `ParseSourceError`）を返すことでこの問題を回避する

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
