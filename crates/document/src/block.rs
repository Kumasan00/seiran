//! ブロックレベル要素とドキュメント全体の型定義

use types::{ColumnAlign, ColumnWidth, HeadingLevel, Length};

use crate::{caption::CaptionPosition, inline::InlineNode, list::ListItem, math::MathNode, table::TableRow};

// =============================================================================
// ドキュメント全体
// =============================================================================

/// ドキュメント全体を表す構造体
///
/// ドキュメント本体のブロック要素を保持します。
/// 目次生成やメタデータ管理にも利用されます。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
  /// ドキュメント本体のブロック要素
  pub body: Vec<DocNode>,
}

impl Document {
  /// ブロックノードのリストから `Document` を生成する
  #[must_use]
  pub fn new(body: Vec<DocNode>) -> Self { return Document { body }; }

  /// ドキュメント内の全見出しを収集する
  ///
  /// 目次生成や PDF ブックマーク構築に使用します。
  ///
  /// # Returns
  ///
  /// `(HeadingLevel, &str, &[InlineNode])` のタプルリスト
  #[must_use]
  pub fn collect_headings(&self) -> Vec<(HeadingLevel, &str, &[InlineNode])> {
    let mut headings = Vec::new();
    for node in &self.body {
      if let DocNode::Heading {
        level,
        number,
        title,
        ..
      } = node
      {
        headings.push((*level, number.as_str(), title.as_slice()));
      }
    }
    return headings;
  }

  /// ドキュメント内のブロック要素の数を返す
  #[must_use]
  pub fn len(&self) -> usize { return self.body.len(); }

  /// ドキュメントが空かどうかを判定する
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.body.is_empty(); }
}

// =============================================================================
// ブロックレベル要素
// =============================================================================

/// ブロックレベルのドキュメント要素
///
/// ドキュメントの最上位に配置される論理的な構造単位です。
/// 各バリアントはセマンティック情報を保持し、
/// 物理的なレイアウト情報（フォントサイズ、座標等）は含みません。
#[derive(Debug, Clone, PartialEq)]
pub enum DocNode {
  /// 見出し（`\part` 〜 `\subparagraph`）
  ///
  /// 番号・レベル・タイトルを構造的に保持し、
  /// スタイル情報は Lowering 層で決定されます。
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 自動採番された見出し番号（`CounterRegistry::increment` の出力に
    /// `format` テンプレートを適用済みの文字列。例: `"1.2"`、`"第3章"`）
    number: String,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
    /// `\section[label=sec:intro]{...}` 形式で付与された参照ラベル（任意）
    ///
    /// `\ref{sec:intro}` 解決時に `CounterRegistry::labels` から番号を引くキーとなる。
    /// `None` の場合はラベル付与なし（参照対象外）。
    label: Option<String>,
  },

  /// 段落（インライン要素の集合）
  ///
  /// 連続するテキスト・インラインコマンドを 1 つの段落にグルーピングしたものです。
  /// `ParagraphBreak` が段落の区切りとなります。
  Paragraph(Vec<InlineNode>),

  /// 箇条書きリスト（`\begin{itemize}` / `\begin{enumerate}`）
  ///
  /// リストアイテムを構造的に保持し、ネスト（リスト内リスト）にも対応可能です。
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<ListItem>,
  },

  /// ディスプレイ数式（`\begin{equation}...\end{equation}`）
  ///
  /// Parser 側で env body が math モードで構造化されるため `Vec<MathNode>` を直接保持する。
  /// 評価時に `CounterRegistry::increment(CounterName::Equation)` で発番された番号
  /// （`format` テンプレ適用済みの文字列）を `number` に保持し、lowering 層が
  /// `EquationStyle::number_format` でさらに装飾を加えて描画する。
  DisplayMath {
    /// 数式本体
    body: Vec<MathNode>,
    /// `\ref` 用ラベル（`[label=eq:foo]`）
    label: Option<String>,
    /// 評価時に発番された通し番号（プレーン文字列）。
    /// 番号が振られない環境（将来の `equation*` 等）では `None`。
    number: Option<String>,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  ///
  /// 環境ハンドラが body 内の `\image` / `\caption` を抽出して構造化する。
  /// `width` / `height` は `\image` の任意引数で mm/cm 単位指定。両方とも省略可で、
  /// 未指定分は `pdf_gen` 段で元画像の自然寸法の縦横比と本文幅から自動算出される。
  /// `number` は `CounterRegistry::increment(CounterName::Figure)` で発番された通し番号
  /// （`format` テンプレ適用済みの文字列）。`caption_position` は
  /// `\caption` が `\image` より前に書かれた場合 `Top`、それ以外は `Bottom`。
  Figure {
    /// 画像ファイルへのパス（`\image{...}` の必須引数）
    image_path: String,
    /// 画像の幅（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
    width: Option<Length>,
    /// 画像の高さ（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
    height: Option<Length>,
    /// `\image[dpi=...]` の per-image 上書き。`None` なら `style.figure.max_dpi` が使われる
    dpi: Option<u32>,
    /// `\image[downsample=...]` の per-image 上書き。`None` なら `style.figure.downsample` が使われる
    downsample: Option<bool>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<InlineNode>>,
    /// キャプションを図本体の上下どちらに配置するか。ソース上の `\caption` / `\image` の
    /// 出現順から決定される
    caption_position: CaptionPosition,
    /// `\ref{fig:foo}` 解決用ラベル（環境の任意引数 `[label=fig:foo]`）
    label: Option<String>,
    /// 評価時に発番された通し番号（プレーン文字列）
    number: String,
  },

  /// 表環境（`\begin{table}...\end{table}`）
  ///
  /// 環境ハンドラが body 内の `\head` / `\row` / `\caption` を抽出して構造化する。
  /// `columns` / `widths` は環境の任意引数 `columns="left center right"` /
  /// `widths="auto auto 5cm"` のパース結果で、長さは列数に揃えられている
  /// （未指定分は `ColumnAlign::Left` / `ColumnWidth::Auto` で埋める）。
  /// `number` は `CounterRegistry::increment(CounterName::Table)` で発番された通し番号。
  /// `caption_position` は `\caption` が最初の行（`\head` / `\row`）より前に
  /// 書かれた場合 `Top`、それ以外は `Bottom`。
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
    /// 評価時に発番された通し番号（プレーン文字列）
    number: String,
    /// 改ページによる分割を許可するか（`[breakable=false]` で禁止、既定 `true`）
    breakable: bool,
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
}

impl DocNode {
  /// 見出しノードを生成するヘルパー（label なし）
  ///
  /// # Arguments
  ///
  /// * `level` - 見出しレベル
  /// * `number` - 書式化済みの見出し番号文字列
  /// * `title` - タイトルのインライン要素
  #[must_use]
  pub fn heading(level: HeadingLevel, number: impl Into<String>, title: Vec<InlineNode>) -> Self {
    return DocNode::Heading {
      level,
      number: number.into(),
      title,
      label: None,
    };
  }

  /// このノードが見出しかどうかを判定する
  #[must_use]
  pub fn is_heading(&self) -> bool { return matches!(self, DocNode::Heading { .. }); }

  /// このノードが段落かどうかを判定する
  #[must_use]
  pub fn is_paragraph(&self) -> bool { return matches!(self, DocNode::Paragraph(_)); }

  /// このノードがリストかどうかを判定する
  #[must_use]
  pub fn is_list(&self) -> bool { return matches!(self, DocNode::List { .. }); }
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  // ==========================================================
  // HeadingLevel のテスト
  // ==========================================================

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

  // ==========================================================
  // DocNode のテスト
  // ==========================================================

  #[test]
  fn doc_node_heading_helper() {
    let node = DocNode::heading(HeadingLevel::Section, "1.2", vec![InlineNode::text("Title")]);
    assert!(node.is_heading());
    assert!(!node.is_paragraph());
    assert!(!node.is_list());
  }

  #[test]
  fn doc_node_is_paragraph() {
    let node = DocNode::Paragraph(vec![InlineNode::text("text")]);
    assert!(node.is_paragraph());
    assert!(!node.is_heading());
  }

  #[test]
  fn doc_node_is_list() {
    let node = DocNode::List {
      ordered: false,
      items: vec![],
    };
    assert!(node.is_list());
    assert!(!node.is_paragraph());
  }

  // ==========================================================
  // Document のテスト
  // ==========================================================

  #[test]
  fn document_new_and_accessors() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::text("Intro")]),
      DocNode::Paragraph(vec![InlineNode::text("Hello")]),
      DocNode::heading(HeadingLevel::Section, "1.1", vec![InlineNode::text("Basics")]),
    ]);

    assert_eq!(doc.len(), 3);
    assert!(!doc.is_empty());
  }

  #[test]
  fn document_collect_headings() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::text("Ch1")]),
      DocNode::Paragraph(vec![InlineNode::text("text")]),
      DocNode::heading(HeadingLevel::Section, "1.1", vec![InlineNode::text("Sec1.1")]),
    ]);

    let headings = doc.collect_headings();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].0, HeadingLevel::Chapter);
    assert_eq!(headings[0].1, "1");
    assert_eq!(headings[1].0, HeadingLevel::Section);
    assert_eq!(headings[1].1, "1.1");
  }

  #[test]
  fn document_empty() {
    let doc = Document::new(vec![]);
    assert!(doc.is_empty());
    assert_eq!(doc.len(), 0);
    assert!(doc.collect_headings().is_empty());
  }
}
