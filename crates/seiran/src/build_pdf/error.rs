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

  /// 脚注のページ単位採番（`[footnote] numbering = "per_page"`）が上限回数の組版で収束しなかった場合
  ///
  /// 番号の桁数変化がマーカー幅を通じてページ割り当てを揺らし続けるケース（脚注が 9 → 10 の桁境界で
  /// ページ境界に乗り続ける等）。一部のページで番号が 1 から始まらない不整合な結果を成功として
  /// 出力せず、回避策付きの診断として報告する。
  #[error("脚注のページ単位採番が {passes} 回の組版で収束しませんでした。")]
  #[diagnostic(
    code(build::footnote::per_page_not_converged),
    help(
      "style.toml の [footnote] を numbering = \"continuous\"（文書通しの採番）に切り替えるか、ページ境界に脚注が集中している箇所の本文量・脚注の長さを調整してください。"
    )
  )]
  PerPageFootnoteNotConverged {
    /// 打ち切った組版パスの回数
    passes: u32,
  },
}
