//! PDF ビルドエラー型の定義

use citation::CitationError;
use config::LayoutValidationError;
use frontend::ParseSourceError;
use miette::{Diagnostic, NamedSource};
use thiserror::Error;
use typeset::LoweringError;

/// PDF ビルド時のエラー型
#[derive(Debug, Error, Diagnostic)]
pub(super) enum BuildPdfError {
  /// テキストファイルの読み込みに失敗した場合
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(build::read_text_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadTextFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 複数ソースのパース・評価で発生したエラーの集約
  ///
  /// 補足設計のエラー戦略: 文法・評価エラーは集約して 1 度に報告する。
  /// 各 `ParseSourceError` は `NamedSource` と内側エラーの label を保持しているため、
  /// `#[related]` 経由で `miette` のフル診断（ソースコード付き）が表示される。
  #[error("複数のソースファイルでエラーが発生しました。")]
  #[diagnostic(code(build::multiple_source_errors))]
  MultipleSourceErrors {
    /// ソースファイルごとのパース・評価エラー
    #[related]
    errors: Vec<ParseSourceError>,
  },

  /// 文献引用（`\cite`）の CSL 整形ステージで発生したエラー
  ///
  /// 内側の [`CitationError`] が持つ `code` / `help` は `#[diagnostic_source]` により外側へ伝播される。
  #[error("文献引用の整形に失敗しました。")]
  #[diagnostic(code(build::citation))]
  Citation {
    /// 元の citation エラー
    #[source]
    #[diagnostic_source]
    source: CitationError,
  },

  /// Document IR → `LayoutNode` 変換（lowering）で発生したエラー（帰属ソースが特定できる場合）
  ///
  /// `LoweringError::source_id()` が指すソースファイルの `NamedSource` を同梱するため、
  /// 未解決参照・重複ラベル等の診断がパースエラーと同じくファイル名・スニペット・下線付きで表示される。
  /// 内側の [`LoweringError`] が持つ `code` / `help` は `#[diagnostic_source]` により外側へ伝播されます。
  #[error("ドキュメントのレイアウト変換に失敗しました。")]
  #[diagnostic(code(build::lowering))]
  Lowering {
    /// エラーが帰属するソースファイルの名前と内容（診断スニペット用）
    #[source_code]
    src: NamedSource<String>,
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// Document IR → `LayoutNode` 変換で発生したが、特定ソースファイルに帰属できないエラー
  ///
  /// 書誌（合成グループ、`parsed` の範囲外の `SourceId`）由来のエラーなど、`NamedSource` を
  /// 特定できないケース用のフォールバック。通常は発生しない（書誌ノードはラベル・`\ref` を持たない）。
  #[error("ドキュメントのレイアウト変換に失敗しました（帰属元ソース不明）。")]
  #[diagnostic(code(build::lowering_internal))]
  LoweringInternal {
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// PDF ファイルの書き込みに失敗した場合
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(build::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// config（用紙・余白）× style（`[columns]`）の横断バリデーションで発生したエラー
  ///
  /// 内側の [`LayoutValidationError`] が持つ `code` / `help` は `#[diagnostic_source]` により外側へ伝播される。
  #[error("ページレイアウトの検証に失敗しました。")]
  #[diagnostic(code(build::layout))]
  Layout {
    /// 元の横断バリデーションエラー
    #[source]
    #[diagnostic_source]
    source: LayoutValidationError,
  },
}
