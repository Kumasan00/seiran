//! PDF ビルドエラー型の定義

use citation::CitationError;
use lowering::LoweringError;
use miette::Diagnostic;
use parser::ParseSourceError;
use thiserror::Error;

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

  /// Document IR → `LayoutNode` 変換（lowering）で発生したエラー
  ///
  /// 内側の [`LoweringError`] が持つ `code` / `help` は `#[diagnostic_source]` により
  /// 外側へ伝播されます。
  #[error("ドキュメントのレイアウト変換に失敗しました。")]
  #[diagnostic(code(build::lowering))]
  Lowering {
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

  /// 段組み設定により 1 段あたりの幅が 0 以下になった場合
  ///
  /// 段幅 = `(本文幅 − (段数 − 1) × 段間) / 段数`。段間が本文幅に対して大きすぎると非正になる。
  /// この制約は config（用紙・余白）と style（`[columns]`）の横断で決まるため `read_style` 単体では
  /// 検証できず、両者が揃うこのステージで判定する。
  #[error(
    "段組みの 1 段あたりの幅が 0 以下になりました（本文幅 {text_width:.1}pt / 段数 {num_columns} / 段間 {column_gap:.1}pt）。"
  )]
  #[diagnostic(
    code(build::invalid_columns),
    help(
      "style.toml の [columns].gap を小さくするか、count を減らしてください。または config.toml の用紙幅を広げる・左右余白を狭めて本文幅を確保してください。"
    )
  )]
  InvalidColumnWidth {
    /// 本文幅（pt）
    text_width: f32,
    /// 段数
    num_columns: usize,
    /// 段間（pt）
    column_gap: f32,
  },
}
