//! ブロックレベル要素とドキュメント全体の型定義

use crate::model::{
  AssetId, CaptionPosition, CitationId, ColumnAlign, ColumnWidth, HeadingLevel, InlineNode, Length, ListItem,
  MathEnvKind, MathRow, QuoteKind, Span, TableRow, TheoremClass,
};

/// ドキュメント全体を表す構造体
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
  /// ドキュメント本体のブロック要素
  pub body: Vec<DocNode>,
}

impl Document {
  /// ブロックノードのリストから `Document` を生成する
  #[must_use]
  pub fn new(body: Vec<DocNode>) -> Self { return Document { body }; }

  /// ドキュメント内のブロック要素の数を返す
  #[must_use]
  pub fn len(&self) -> usize { return self.body.len(); }

  /// ドキュメントが空かどうかを判定する
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.body.is_empty(); }
}

/// ブロックレベルのドキュメント要素
///
/// セマンティック情報のみを保持し、フォントサイズや座標などの物理レイアウトは含まない。
#[derive(Debug, Clone, PartialEq)]
pub enum DocNode {
  /// 見出し（`\part` 〜 `\subparagraph`）
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 採番対象かどうか。`true` なら `lowering` 層が対応するカウンタを発番する。
    ///
    /// 通常の見出しコマンド（`\section` 等）は常に `true`。CSL 整形ステージ（`citation` クレート）が
    /// 合成する「References」見出しのように、frontend を経由せず直接構築される見出しは `false` になる。
    numbered: bool,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
    /// `\section[label=sec:intro]{...}` 形式で付与された参照ラベル
    label: Option<String>,
    /// 見出しコマンドのソース位置。重複ラベルの診断に使う
    span: Span,
  },

  /// 段落（インライン要素の集合）
  Paragraph(Vec<InlineNode>),

  /// 箇条書きリスト（`\begin{itemize}` / `\begin{enumerate}`）
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<ListItem>,
    /// 開始番号（`enumerate[start=N]`）。`None` は既定（1 から開始）。
    /// `itemize`（unordered）では常に `None`
    start: Option<u32>,
    /// 項目間の縦アキの上書き。`None` は style.toml の既定値
    item_gap: Option<Length>,
  },

  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  ///
  /// `equation` / `align` / `gather` は行単位、`split` / `multiline` は環境単位で採番する。
  /// `cases` / `matrix` は採番しない。ラベルも同じ粒度で保持する。
  MathBlock {
    /// 環境種別
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列を持つ）
    rows: Vec<MathRow>,
    /// 環境全体で 1 つ採番するか（`split` / `multiline` 用）。行ごと採番の環境（`equation` /
    /// `align` / `gather`）や `cases` / `matrix` では意味を持たない（常に `false`）
    numbered: bool,
    /// `\ref{eq:foo}` 解決用の環境単位ラベル（`split` / `multiline` の `[label=...]`）。
    /// 行ごと採番の環境（ラベルは `MathRow::label` 側）や無採番では `None`
    label: Option<String>,
    /// 環境のソース位置。重複ラベルの診断や行ラベルの位置未指定時のフォールバックに使う
    span: Span,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  Figure {
    /// 画像ファイルへのパス（`\image{...}` の必須引数）
    image_path: AssetId,
    /// 画像の幅（未指定の場合は `seiran_pdf` 段で本文幅 / 縦横比から算出）
    width: Option<Length>,
    /// 画像の高さ（未指定の場合は `seiran_pdf` 段で本文幅 / 縦横比から算出）
    height: Option<Length>,
    /// `\image[dpi=...]` の per-image 上書き。`None` なら config `[image].max_dpi` が使われる
    dpi: Option<u32>,
    /// `\image[downsample=...]` の per-image 上書き。`None` なら config `[image].downsample` が使われる
    downsample: Option<bool>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<InlineNode>>,
    /// キャプションを図本体の上下どちらに配置するか。ソース上の `\caption` / `\image` の
    /// 出現順から決定される
    caption_position: CaptionPosition,
    /// `\ref{fig:foo}` 解決用ラベル（環境の任意引数 `[label=fig:foo]`）
    label: Option<String>,
    /// 環境のソース位置。重複ラベルの診断に使う
    span: Span,
  },

  /// 表環境（`\begin{table}...\end{table}`）
  Table {
    /// 列ごとの揃え方向（列数に正規化済み）
    columns: Vec<ColumnAlign>,
    /// 列ごとの幅指定（列数に正規化済み）
    widths: Vec<ColumnWidth>,
    /// ヘッダ行（`\head{...}` 内の `\row`）。改ページ時に再描画される
    head: Vec<TableRow>,
    /// 本体行（`\row{...}`）
    rows: Vec<TableRow>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<InlineNode>>,
    /// キャプションを表本体の上下どちらに配置するか
    caption_position: CaptionPosition,
    /// `\ref{tab:foo}` 解決用ラベル（環境の任意引数 `[label=tab:foo]`）
    label: Option<String>,
    /// 環境のソース位置。重複ラベルの診断に使う
    span: Span,
    /// 改ページによる分割を許可するか（`[breakable=false]` で禁止、既定 `true`）
    breakable: bool,
  },

  /// 定理ブロック（`\begin{theorem}...\end{theorem}` 等の 10 種）
  Theorem {
    /// 定理クラス（`theorem` / `lemma` / … / `proof`）。`lowering` がスタイル解決に使う
    class: TheoremClass,
    /// サブタイトル（`[title="..."]` の中身）。見出しの `{title}` に反映される。未指定は `None`
    title: Option<String>,
    /// 本体（再帰評価された `Vec<DocNode>`）
    body: Vec<DocNode>,
    /// `proof` の `[of=label]` 参照（証明対象の定理）。`proof` 以外や未指定は `None`
    of: Option<ProofTarget>,
    /// `\ref{thm:foo}` 解決用ラベル（環境の任意引数 `[label=thm:foo]`）。未指定は `None`
    label: Option<String>,
    /// 環境のソース位置。重複ラベルの診断に使う
    span: Span,
  },

  /// 引用ブロック（`\begin{quote}...\end{quote}` / `\begin{quotation}...\end{quotation}`）
  Quote {
    /// 引用の種別（`quote` / `quotation`）。`lowering` が段落先頭字下げの有無に使う
    kind: QuoteKind,
    /// 本体（再帰評価された `Vec<DocNode>`）
    body: Vec<DocNode>,
  },

  /// 罫線（描画線）
  Rule {
    /// 幅
    width: Length,
    /// 高さ
    height: Length,
  },

  /// 改ページ
  PageBreak,

  /// 固定幅スペース（`\space{N}` コマンド、pt 単位）
  Space(Length),

  /// 参考文献エントリに置くゼロサイズの参照アンカー
  Anchor(CitationId),
}

/// `proof` 環境の `[of=label]` 参照（証明対象の定理）
#[derive(Debug, Clone, PartialEq)]
pub struct ProofTarget {
  /// 参照先のラベル名（`[of=thm:foo]` の `thm:foo`）
  pub label: String,
  /// `[of=...]` 任意引数のソース位置。未解決時の診断に使う
  pub span: Span,
}

impl DocNode {
  /// 採番あり・ラベルなしの見出しノードを生成する
  #[must_use]
  pub fn heading(level: HeadingLevel, title: Vec<InlineNode>) -> Self {
    return DocNode::Heading {
      level,
      numbered: true,
      title,
      label: None,
      span: Span::DUMMY,
    };
  }

  /// このノードが段落かどうかを判定する
  #[must_use]
  pub fn is_paragraph(&self) -> bool { return matches!(self, DocNode::Paragraph(_)); }

  /// このノードがリストかどうかを判定する
  #[must_use]
  pub fn is_list(&self) -> bool { return matches!(self, DocNode::List { .. }); }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn heading_level_depth_returns_correct_values() {
    assert_eq!(HeadingLevel::Part.depth(), 0);
    assert_eq!(HeadingLevel::Chapter.depth(), 1);
    assert_eq!(HeadingLevel::Section.depth(), 2);
    assert_eq!(HeadingLevel::Subsection.depth(), 3);
    assert_eq!(HeadingLevel::Paragraph.depth(), 4);
    assert_eq!(HeadingLevel::Subparagraph.depth(), 5);
  }

  #[test]
  fn heading_level_ordering() {
    assert!(HeadingLevel::Part < HeadingLevel::Chapter);
    assert!(HeadingLevel::Chapter < HeadingLevel::Section);
    assert!(HeadingLevel::Section < HeadingLevel::Subsection);
    assert!(HeadingLevel::Subsection < HeadingLevel::Paragraph);
    assert!(HeadingLevel::Paragraph < HeadingLevel::Subparagraph);
  }

  #[test]
  fn heading_level_from_command_name() {
    assert_eq!(HeadingLevel::from_command_name("part"), Some(HeadingLevel::Part));
    assert_eq!(HeadingLevel::from_command_name("chapter"), Some(HeadingLevel::Chapter));
    assert_eq!(HeadingLevel::from_command_name("section"), Some(HeadingLevel::Section));
    assert_eq!(HeadingLevel::from_command_name("subsection"), Some(HeadingLevel::Subsection));
    assert_eq!(HeadingLevel::from_command_name("paragraph"), Some(HeadingLevel::Paragraph));
    assert_eq!(HeadingLevel::from_command_name("subparagraph"), Some(HeadingLevel::Subparagraph));
    assert_eq!(HeadingLevel::from_command_name("unknown"), None);
  }

  #[test]
  fn heading_level_command_name() {
    assert_eq!(HeadingLevel::Part.command_name(), "part");
    assert_eq!(HeadingLevel::Chapter.command_name(), "chapter");
    assert_eq!(HeadingLevel::Section.command_name(), "section");
    assert_eq!(HeadingLevel::Subsection.command_name(), "subsection");
    assert_eq!(HeadingLevel::Paragraph.command_name(), "paragraph");
    assert_eq!(HeadingLevel::Subparagraph.command_name(), "subparagraph");
  }

  #[test]
  fn heading_level_display() {
    assert_eq!(format!("{}", HeadingLevel::Section), "section");
    assert_eq!(format!("{}", HeadingLevel::Part), "part");
  }

  #[test]
  fn doc_node_heading_helper() {
    let node = DocNode::heading(HeadingLevel::Section, vec![InlineNode::text("Title")]);
    assert!(!node.is_paragraph());
    assert!(!node.is_list());
  }

  #[test]
  fn doc_node_is_paragraph() {
    let node = DocNode::Paragraph(vec![InlineNode::text("text")]);
    assert!(node.is_paragraph());
  }

  #[test]
  fn doc_node_is_list() {
    let node = DocNode::List {
      ordered: false,
      items: vec![],
      start: None,
      item_gap: None,
    };
    assert!(node.is_list());
    assert!(!node.is_paragraph());
  }

  #[test]
  fn document_new_and_accessors() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, vec![InlineNode::text("Intro")]),
      DocNode::Paragraph(vec![InlineNode::text("Hello")]),
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::text("Basics")]),
    ]);

    assert_eq!(doc.len(), 3);
    assert!(!doc.is_empty());
  }

  #[test]
  fn document_empty() {
    let doc = Document::new(vec![]);
    assert!(doc.is_empty());
    assert_eq!(doc.len(), 0);
  }
}
