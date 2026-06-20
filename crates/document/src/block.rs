//! ブロックレベル要素とドキュメント全体の型定義

use types::{ColumnAlign, ColumnWidth, HeadingLevel, Length, MathEnvKind};

use crate::{caption::CaptionPosition, inline::InlineNode, list::ListItem, math::MathRow, table::TableRow};

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

  /// ドキュメント内の全見出しを文書順に収集する
  ///
  /// 目次生成や PDF ブックマーク構築に使用します。意味側の単一ソースである
  /// 自由関数 [`collect_headings`] に委譲します。
  #[must_use]
  pub fn collect_headings(&self) -> Vec<HeadingInfo<'_>> { return collect_headings(&self.body); }

  /// ドキュメント内のブロック要素の数を返す
  #[must_use]
  pub fn len(&self) -> usize { return self.body.len(); }

  /// ドキュメントが空かどうかを判定する
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.body.is_empty(); }
}

// =============================================================================
// 見出しの収集（目次生成・PDF しおりの共通ソース）
// =============================================================================

/// 文書順インデックス付きの見出しビュー
///
/// [`collect_headings`] が返す軽量な参照ビュー。目次生成と PDF しおり構築の双方が
/// この単一ソースを消費する。`index` は本文中の見出し出現順（0 始まり）で、
/// [`heading_anchor_key`] に渡すと目次エントリの内部リンク到達先キーが得られる。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadingInfo<'a> {
  /// 見出しの文書順インデックス（0 始まり）
  pub index: usize,
  /// 見出しレベル
  pub level: HeadingLevel,
  /// 書式化済みの見出し番号
  pub number: &'a str,
  /// 見出しタイトル（インライン要素）
  pub title: &'a [InlineNode],
}

/// 見出しの暗黙 destination キーを文書順インデックスから生成する
///
/// lowering（採番側 = `AnchorMark::Heading.key`）と目次ビルダ（参照側 =
/// `LinkTarget::Internal`）が同じ規則を共有するための単一ソース。
#[must_use]
pub fn heading_anchor_key(index: usize) -> String { return format!("heading:{index}"); }

/// ブロックノード列から全見出しを文書順に収集する
///
/// 目次生成と PDF しおり構築が共通で使う意味側の単一ソース。`index` は出現順に
/// 0 から振られる。
#[must_use]
pub fn collect_headings(nodes: &[DocNode]) -> Vec<HeadingInfo<'_>> {
  let mut headings = Vec::new();
  for node in nodes {
    if let DocNode::Heading {
      level,
      number,
      title,
      ..
    } = node
    {
      headings.push(HeadingInfo {
        index: headings.len(),
        level: *level,
        number: number.as_str(),
        title: title.as_slice(),
      });
    }
  }
  return headings;
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

  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  ///
  /// Parser が環境種別を `kind` に決め、本体を `\\` で行・`&` で列に分割して `rows` に
  /// 格納する（env body は math モードで構造化される）。採番には 2 つの粒度があり、kind ごとに
  /// **どちらか一方だけ**を使う:
  ///
  /// - **行ごと採番**（`equation` / `align` / `gather`）: 各行の `MathRow::number` に通し番号が入る。
  ///   環境全体の `number` は `None`。
  /// - **環境全体に 1 つ採番**（`split` / `multiline`）: この `number` フィールドに 1 つだけ通し番号が入り、
  ///   `layout` 段がブロックの縦中央に配置する。各行の `MathRow::number` は `None`。
  ///
  /// いずれも番号は `CounterRegistry::increment(CounterName::Equation)` で発番された通し番号
  /// （`format` テンプレ適用済みの文字列）で、lowering 層が `EquationStyle::number_format` でさらに
  /// 装飾する。`[numbered=false]` で採番ありの環境を無採番にした場合は両方とも `None`。`kind` に応じて
  /// lowering 以降が列整列・区切り括弧・中央寄せを決める。
  MathBlock {
    /// 環境種別
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列を持つ）
    rows: Vec<MathRow>,
    /// 環境全体に 1 つだけ付く通し番号（`split` / `multiline` 用、縦中央配置）。
    /// 行ごと採番の環境（`equation` / `align` / `gather`）や無採番では `None`
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

  /// ゼロサイズの参照アンカー点（指定キーで参照可能な位置）
  ///
  /// それ自身は縦アキを生まず、`lowering` 層で `LayoutNode::Anchor(AnchorMark::Label(key))` に
  /// 変換され、直後のブロックの確定座標にジャンプ先として解決される。見出し・図・表・式の
  /// ラベルは各 `DocNode` の `label` フィールドが担うため、これは本文構造に label フィールドを
  /// 持たないブロック（CSL 整形ステージが追加する参考文献エントリ段落）にアンカーを付けるための
  /// プリミティブ。キーは衝突回避のため `"cite:<引用キー>"` の形で名前空間化される。
  Anchor(String),
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
    assert_eq!(headings[0].index, 0);
    assert_eq!(headings[0].level, HeadingLevel::Chapter);
    assert_eq!(headings[0].number, "1");
    assert_eq!(headings[1].index, 1);
    assert_eq!(headings[1].level, HeadingLevel::Section);
    assert_eq!(headings[1].number, "1.1");
  }

  #[test]
  fn document_empty() {
    let doc = Document::new(vec![]);
    assert!(doc.is_empty());
    assert_eq!(doc.len(), 0);
    assert!(doc.collect_headings().is_empty());
  }
}
