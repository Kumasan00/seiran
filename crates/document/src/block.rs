//! ブロックレベル要素とドキュメント全体の型定義

use miette::SourceSpan;
use types::{ColumnAlign, ColumnWidth, HeadingLevel, Length, MathEnvKind, TheoremClass};

use crate::{
  caption::CaptionPosition, inline::InlineNode, list::ListItem, math::MathRow, quote::QuoteKind, table::TableRow,
};

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

  /// ドキュメント内のブロック要素の数を返す
  #[must_use]
  pub fn len(&self) -> usize { return self.body.len(); }

  /// ドキュメントが空かどうかを判定する
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.body.is_empty(); }
}

// =============================================================================
// 見出しの暗黙 destination キー
// =============================================================================

/// 見出しの暗黙 destination キーを文書順インデックスから生成する
///
/// lowering（採番側 = `AnchorMark::Heading.key`）と目次ビルダ（参照側 =
/// `LinkTarget::Internal`）が同じ規則を共有するための単一ソース。見出しの収集自体は
/// `lowering::lower_sources_with_headings`（採番・書式解決を伴うため lowering 側の責務）が担う。
#[must_use]
pub fn heading_anchor_key(index: usize) -> String { return format!("heading:{index}"); }

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
    /// 採番対象かどうか。`true` なら `lowering` 層が対応するカウンタを発番する。
    ///
    /// 通常の見出しコマンド（`\section` 等）は常に `true`。CSL 整形ステージ（`citation` クレート）が
    /// 合成する「References」見出しのように、parser を経由せず直接構築される見出しは `false` になる。
    numbered: bool,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
    /// `\section[label=sec:intro]{...}` 形式で付与された参照ラベル（任意）
    ///
    /// `\ref{sec:intro}` 解決時に `lowering::CounterRegistry` の labels から番号を引くキーとなる。
    /// `None` の場合はラベル付与なし（参照対象外）。
    label: Option<String>,
    /// 見出しコマンドのソース位置。重複ラベルの診断に使う
    span: SourceSpan,
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
  /// - **行ごと採番**（`equation` / `align` / `gather`）: 各行の `MathRow::numbered` が採番対象かを表す。
  ///   環境全体の採番（この `numbered` フィールド）は使わない。
  /// - **環境全体に 1 つ採番**（`split` / `multiline`）: この `numbered` フィールドが `true` かつ
  ///   `rows` が空でなければ、`lowering` 層が環境全体に 1 つだけ通し番号を発番し、ブロックの縦中央に
  ///   配置する。各行の `MathRow::numbered` は常に `false`。
  ///
  /// `cases` / `matrix` は非採番（`numbered: false` 固定）で `CounterName::Equation` を一切消費しない。
  /// 番号の書式化（`number_format` テンプレ・`MathBlockStyle::tag_format` による装飾）は
  /// すべて `lowering` 層が `Style` を参照して行う。`kind` に応じて lowering 以降が列整列・
  /// 区切り括弧・中央寄せを決める。
  ///
  /// ラベルの担い手も採番粒度に揃える。行ごと採番（`equation` / `align` / `gather`）は各行の
  /// `MathRow::label`（`align` / `gather` は行末マーカー `\label{...}`、`equation` は `[label=...]`）が、
  /// 環境単位採番（`split` / `multiline`）は環境の任意引数 `[label=...]` を受けるこの `label` フィールドが
  /// 担う。lowering 層がいずれも `AnchorMark::Label` でブロック先頭の `\ref` 到達先に解決する。
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
    span: SourceSpan,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  ///
  /// 環境ハンドラが body 内の `\image` / `\caption` を抽出して構造化する。
  /// `width` / `height` は `\image` の任意引数で mm/cm 単位指定。両方とも省略可で、
  /// 未指定分は `pdf_gen` 段で元画像の自然寸法の縦横比と本文幅から自動算出される。
  /// 通し番号（`CounterName::Figure`）の発番・書式化は `lowering` 層が担う。`caption_position` は
  /// `\caption` が `\image` より前に書かれた場合 `Top`、それ以外は `Bottom`。
  Figure {
    /// 画像ファイルへのパス（`\image{...}` の必須引数）
    image_path: String,
    /// 画像の幅（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
    width: Option<Length>,
    /// 画像の高さ（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
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
    span: SourceSpan,
  },

  /// 表環境（`\begin{table}...\end{table}`）
  ///
  /// 環境ハンドラが body 内の `\head` / `\row` / `\caption` を抽出して構造化する。
  /// `columns` / `widths` は環境の任意引数 `columns="left center right"` /
  /// `widths="auto auto 5cm"` のパース結果で、長さは列数に揃えられている
  /// （未指定分は `ColumnAlign::Left` / `ColumnWidth::Auto` で埋める）。
  /// 通し番号（`CounterName::Table`）の発番・書式化は `lowering` 層が担う。
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
    /// 環境のソース位置。重複ラベルの診断に使う
    span: SourceSpan,
    /// 改ページによる分割を許可するか（`[breakable=false]` で禁止、既定 `true`）
    breakable: bool,
  },

  /// 定理ブロック（`\begin{theorem}...\end{theorem}` 等の 10 種）
  ///
  /// 環境ハンドラがクラスをを解決し、`[title=...]` / `[label=...]` / `[of=...]` の任意引数を
  /// 抽出して構造化する。本体（`body`）は通常の本文と同様に再帰評価された `Vec<DocNode>`。
  /// 採番（共有カウンタの発番・cleveref 文字列の組み立て）・見出し書式・本文フォント・
  /// QED マーク配置は、いずれも `lowering` 層がクラスの `read_style::TheoremStyle` を参照して
  /// 決める（このノードは `class` 以外に物理・書式情報を持たない）。`unnumbered` クラス（`proof`）
  /// かどうかも `TheoremStyle.unnumbered` から `lowering` が判定する。
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
    span: SourceSpan,
  },

  /// 引用ブロック（`\begin{quote}...\end{quote}` / `\begin{quotation}...\end{quotation}`）
  ///
  /// 本文より左右に字下げされたブロック引用。本体（`body`）は通常の本文と同様に再帰評価された
  /// `Vec<DocNode>`（段落・リスト・数式などを含められる）。`kind` が `Quotation` のときブロック内
  /// 段落の先頭行を字下げし、`Quote` のときは字下げしない。左右インデント量・上下マージン・
  /// 段落先頭字下げ量・本文フォントは `lowering` 層が `read_style::QuoteStyle` を参照して決める
  /// （このノードは物理スタイルを持たない）。
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

  /// ゼロサイズの参照アンカー点（指定キーで参照可能な位置）
  ///
  /// それ自身は縦アキを生まず、`lowering` 層で `LayoutNode::Anchor(AnchorMark::Label(key))` に
  /// 変換され、直後のブロックの確定座標にジャンプ先として解決される。見出し・図・表・式の
  /// ラベルは各 `DocNode` の `label` フィールドが担うため、これは本文構造に label フィールドを
  /// 持たないブロック（CSL 整形ステージが追加する参考文献エントリ段落）にアンカーを付けるための
  /// プリミティブ。キーは衝突回避のため `"cite:<引用キー>"` の形で名前空間化される。
  Anchor(String),
}

/// `proof` 環境の `[of=label]` 参照（証明対象の定理）
///
/// `\ref` と同じく `lowering` 層の 2 パスで解決する（`label` を保持するだけの構造体で、
/// 解決結果の cleveref 文字列（例 `"Theorem 1.2"`）は `lowering::resolve_refs` が
/// `LayoutNode` 側で埋め込む）。
#[derive(Debug, Clone, PartialEq)]
pub struct ProofTarget {
  /// 参照先のラベル名（`[of=thm:foo]` の `thm:foo`）
  pub label: String,
  /// `[of=...]` 任意引数のソース位置。未解決時の診断に使う
  pub span: SourceSpan,
}

impl DocNode {
  /// 採番ありの見出しノードを生成するヘルパー（label なし、テスト用ダミー span）
  ///
  /// # Arguments
  ///
  /// * `level` - 見出しレベル
  /// * `title` - タイトルのインライン要素
  #[must_use]
  pub fn heading(level: HeadingLevel, title: Vec<InlineNode>) -> Self {
    return DocNode::Heading {
      level,
      numbered: true,
      title,
      label: None,
      span: SourceSpan::from((0_usize, 0_usize)),
    };
  }

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

  #[test]
  fn heading_anchor_key_formats_index() {
    assert_eq!(heading_anchor_key(0), "heading:0");
    assert_eq!(heading_anchor_key(3), "heading:3");
  }
}
