//! Lowering 層: 意味解析の成果物（`semantics::SemanticDocument`）→ `LayoutNode` 変換
//!
//! ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` が済ませているため、
//! この層は「確定した構造値を style の表示側フィールドで文字列にして箱に積む」だけを行う。
//!
//! 著者が書いた本文は HIR（`document::hir`）を走査し、事実は `NodeId` をキーに [`LoweringState`] の
//! query で引く。CSL 整形の生成物（書誌・引用表示）は `NodeId` を持たないので、別経路
//! （子 module `generated`）で lower する。
//! 段落の内容と段落後の固定アキは [`ParagraphLayout`] に閉じ、boxing へ明示した境界を渡す。

mod code;
mod counter;
mod figure;
mod float;
mod flush;
mod generated;
mod heading;
mod inline;
mod layout_node;
mod list;
mod math;
mod paragraph;
mod quote;
mod table;
mod theorem;
mod title_page;

use std::mem;

pub(super) use layout_node::{
  AtomNode, DelimiterGlyphs, InlineNode, LayoutNode, MathAccent, MathArray, MathBar, MathBlockCellLayout,
  MathBlockLayout, MathFenced, MathFraction, MathOperator, MathRadical, MathScripts, ParagraphLayout, TableLayout,
  TableRowLayout, TextStyle,
};
pub(super) use math::{
  CASES_DELIMITERS, delimiter_glyphs, math_text_clusters, matrix_delimiter_glyphs, translate_math_char,
};
pub(crate) use title_page::{TitlePageMetadata, lower_title_page};
use tracing::debug;

use crate::{
  document::{HeadingLevel, HirNode, HirNodeKind, NodeId, NodeMap, Typeface},
  length::Length,
  project::config::ImageConfig,
  semantics::{BibliographyEntry, CounterValue, GeneratedInline, HeadingKey, LabelId, SemanticDocument},
  style::Style as ReadStyle,
  typeset::{boxes::AnchorId, font::ScriptScale},
};

/// Lowering のコンテキスト
#[derive(Debug, Clone, Copy)]
pub(super) struct LoweringContext<'a> {
  /// スタイル設定への参照
  pub style: &'a ReadStyle,
  /// 本文段落の既定書体
  pub body_typeface: Typeface,
  /// 段落先頭行の字下げ量
  pub first_line_indent: Length,
  /// ラスタ画像埋め込み時の最大 DPI（config `[image].max_dpi` 由来）
  pub image_max_dpi: u32,
  /// ラスタ画像のダウンサンプリング可否（config `[image].downsample` 由来）
  pub image_downsample: bool,
  /// 箇条書き（`itemize` / `enumerate`）のネスト深さ（0 = 最上位）
  pub list_depth: usize,
  /// 脚注の表示番号の上書きマップ（出現 index 引き）。`None` は文書通しの連番
  pub footnote_numbers: Option<&'a [u32]>,
  /// 数式のスクリプト段の縮小率（数式フォントの MATH の値。lowering はフォント資源に触れず値だけを受け取る）
  pub script_scale: ScriptScale,
}

impl<'a> LoweringContext<'a> {
  /// スタイル・検証済みの画像設定（config `[image]`）・数式フォントの縮小率から文脈を生成する
  #[must_use]
  pub(super) fn new(style: &'a ReadStyle, image: ImageConfig, script_scale: ScriptScale) -> Self {
    return LoweringContext {
      style,
      body_typeface: style.text.typeface,
      first_line_indent: style.text.first_line_indent,
      image_max_dpi: image.max_dpi,
      image_downsample: image.downsample,
      list_depth: 0,
      footnote_numbers: None,
      script_scale,
    };
  }

  /// 脚注の表示番号の上書きマップを与えた文脈を返す
  #[must_use]
  pub(super) fn with_footnote_numbers(self, numbers: &'a [u32]) -> Self {
    return LoweringContext {
      footnote_numbers: Some(numbers),
      ..self
    };
  }

  /// 本文段落の既定書体だけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_body_typeface(self, body_typeface: Typeface) -> Self {
    return LoweringContext {
      body_typeface,
      ..self
    };
  }

  /// 段落先頭行の字下げ量だけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_first_line_indent(self, first_line_indent: Length) -> Self {
    return LoweringContext {
      first_line_indent,
      ..self
    };
  }

  /// 箇条書きのネスト深さだけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_list_depth(self, list_depth: usize) -> Self { return LoweringContext { list_depth, ..self }; }

  /// 既定フォントサイズ（段落本文用）を返す
  #[must_use]
  pub(super) fn default_font_size(&self) -> Length { return self.style.text.font_size; }
}

/// 見出し 1 件の記録
#[derive(Debug, Clone, PartialEq)]
pub(super) struct HeadingRecord {
  /// 目次の行のはみ出し警告が指す見出し。書誌の見出し（生成物）は `None`
  pub node: Option<NodeId>,
  /// 見出しの文書順インデックス（0 始まり）
  pub index: usize,
  /// 見出しレベル
  pub level: HeadingLevel,
  /// 書式化済みの見出し番号（無採番の見出しは空文字列）
  pub number: String,
  /// 見出しタイトルのプレーンテキスト（`\ref` 解決済み）
  pub title_plain: String,
}

impl HeadingRecord {
  /// 目次の項目としおりに表示する「番号 タイトル」を組む
  #[must_use]
  pub(in crate::typeset) fn label(&self) -> String {
    if self.number.is_empty() {
      return self.title_plain.clone();
    }
    if self.title_plain.is_empty() {
      return self.number.clone();
    }
    return format!("{} {}", self.number, self.title_plain);
  }
}

/// lowering のテスト入力を組み立てるヘルパ
#[cfg(test)]
pub(super) mod test_support {
  use super::{
    InlineNode, LayoutNode, LoweringContext, NodeId, ParagraphLayout, TextStyle, lower_sources_with_headings,
  };
  use crate::{
    document::HirDocument,
    frontend::test_support::parse_for_test,
    project::config::ImageConfig,
    semantics::{SemanticDocument, SemanticPolicy, analyze_for_test, test_support::sample_references},
    source::SourceId,
    style::Style,
    typeset::font::ScriptScale,
  };

  /// `.sei` スニペットを parse → analyze して意味解析済みドキュメントを作る
  ///
  /// 参照定義は文献フィクスチャ（`kwan2014` / `doe2020`）で、引用の表示・書誌は持たない。
  pub(crate) fn analyzed(source: &str) -> SemanticDocument {
    let hir = HirDocument::assemble(vec![parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず")]);
    return analyze_for_test(hir, &SemanticPolicy::from_style(&Style::default()), &sample_references())
      .expect("解析できる入力のはず");
  }

  /// テストの数式フォント（STIX Two Math）の縮小率（`ScriptPercentScaleDown` 70 / `ScriptScriptPercentScaleDown` 55）
  pub(super) fn stix_script_scale() -> ScriptScale { return ScriptScale::from_percents(70, 55); }

  /// テスト既定の画像設定で lowering の文脈を作る
  ///
  /// 値は config.toml の `[image]` 未指定時の既定（`RawImageConfig::default()`）と同じ。
  pub(super) fn context(style: &Style) -> LoweringContext<'_> {
    return LoweringContext::new(
      style,
      ImageConfig {
        max_dpi: 300,
        downsample: true,
      },
      stix_script_scale(),
    );
  }

  /// 意味解析済みドキュメントを lower してレイアウトノード列を返す
  pub(crate) fn lower(style: &Style, document: &SemanticDocument) -> Vec<LayoutNode> {
    let (layout, _headings) = lower_sources_with_headings(&context(style), document);
    return layout;
  }

  /// 段落ノードの内容と段落後のアキを取り出す
  pub(super) fn paragraph_layout(node: &LayoutNode) -> &ParagraphLayout {
    let LayoutNode::Paragraph(paragraph) = node else {
      panic!("段落が期待されます: {node:?}");
    };
    return paragraph;
  }

  /// 直下の段落のインライン列を出現順に辿る（VBox 内には降りない）
  pub(super) fn inlines(nodes: &[LayoutNode]) -> impl DoubleEndedIterator<Item = &InlineNode> {
    return nodes.iter().flat_map(|node| match node {
      LayoutNode::Paragraph(paragraph) => return paragraph.content.as_slice(),
      _ => return &[],
    });
  }

  /// 段落の起源を文書順に集める（`VBox` の中へも降りる）
  pub(super) fn paragraph_origins(nodes: &[LayoutNode]) -> Vec<Option<NodeId>> {
    let mut origins = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Paragraph(paragraph) => origins.push(paragraph.origin),
        LayoutNode::VBox { children, .. } => origins.extend(paragraph_origins(children)),
        _ => {},
      }
    }
    return origins;
  }

  /// インラインの `Text` なら、その文字列とスタイルを借りる
  pub(super) fn inline_text(node: &InlineNode) -> Option<(&str, TextStyle)> {
    return match node {
      InlineNode::Text(text, style) => Some((text.as_str(), *style)),
      _ => None,
    };
  }
}

/// 走査中に更新される可変状態と、事実を引く query の窓口
pub(super) struct LoweringState<'a> {
  /// 意味解析の成果物（HIR + 事実 + CSL 生成物）
  document: &'a SemanticDocument,
  /// これまでに払い出した脚注の個数（次の脚注の出現 index になる）
  footnote_count: u32,
  /// 見出しノード → タイトルのプレーンテキスト
  heading_titles: NodeMap<String>,
}

impl<'a> LoweringState<'a> {
  /// 入力に対する初期状態を作る
  pub(super) fn new(document: &'a SemanticDocument) -> Self {
    return LoweringState {
      document,
      footnote_count: 0,
      heading_titles: NodeMap::default(),
    };
  }

  /// 脚注を 1 つ数え、その出現 index（0 起点）を返す
  pub(super) fn next_footnote_index(&mut self) -> u32 {
    let index = self.footnote_count;
    self.footnote_count += 1;
    return index;
  }

  /// 引用箇所の表示インライン列を引く
  pub(super) fn citation_display(&self, site: NodeId) -> &'a [GeneratedInline] {
    return self.document.citation_display(site);
  }

  /// `\ref` / `proof` の `[of=...]` の参照先表示文字列を作る
  ///
  /// # Panics
  ///
  /// 参照先のカウンタ値が事実に無い場合にパニックします（`analyze` の存在検証を通過した
  /// `LabelId` しか到達しないため、通常は起こりません）。
  pub(super) fn ref_display(&self, style: &ReadStyle, target: &LabelId) -> String {
    let Some(value) = self.document.counter_value_of_label(target) else {
      unreachable!("参照先の存在は semantics::analyze が保証している: {target:?}")
    };
    return counter::format_ref_display(style, value);
  }

  /// 採番対象ノードのカウンタ構造値を引く（採番対象でなければ `None`）
  pub(super) fn counter_value(&self, node: NodeId) -> Option<&'a CounterValue> {
    return self.document.counter_value(node);
  }

  /// 見出しノードの文書順キーを引く
  pub(super) fn heading_key(&self, node: NodeId) -> HeadingKey { return self.document.heading_key(node); }

  /// ノードが宣言したラベルを引く（ラベルを持たないノードは `None`）
  pub(super) fn declared_label(&self, node: NodeId) -> Option<&'a LabelId> {
    return self.document.declared_label(node);
  }

  /// 参照箇所（`\ref` / `[of=...]`）の参照先を引く
  pub(super) fn ref_target(&self, site: NodeId) -> &'a LabelId { return self.document.ref_target(site); }

  /// 見出しタイトルのプレーンテキストを記録する
  pub(super) fn record_heading_title(&mut self, node: NodeId, plain: String) {
    self.heading_titles.insert(node, plain);
    return;
  }

  /// 記録済みの見出しタイトルのプレーンテキストを引く
  ///
  /// # Panics
  ///
  /// 走査で記録していない見出しノードを渡した場合にパニックします。
  pub(super) fn heading_title(&self, node: NodeId) -> &str {
    let Some(title) = self.heading_titles.get(node) else {
      unreachable!("見出しのタイトルは HIR の走査で必ず記録される: {node:?}")
    };
    return title;
  }
}

/// 意味解析の成果物をレイアウトノードに変換し、見出し記録も返す
#[must_use]
pub(super) fn lower_sources_with_headings(
  ctx: &LoweringContext<'_>,
  document: &SemanticDocument,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let mut state = LoweringState::new(document);
  let mut result = lower_nodes(ctx, document.hir().source_nodes().iter().flatten(), &mut state);

  // 書誌は本文の後ろに置き、見出しキーは本文の見出し数の続きから振る。
  let (bibliography_nodes, bibliography_headings) =
    generated::lower_bibliography(ctx, document.bibliography(), document.heading_count());
  result.extend(bibliography_nodes);

  // 見出し一覧は facts の順（= `analyze` が振った `HeadingKey` の順）で組む。走査順に依存しない。
  let mut headings: Vec<HeadingRecord> = document
    .headings()
    .map(|heading| {
      return HeadingRecord {
        node: Some(heading.node),
        index: heading.key.index(),
        level: heading.level,
        number: document
          .counter_value(heading.node)
          .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value)),
        title_plain: state.heading_title(heading.node).to_string(),
      };
    })
    .collect();
  headings.extend(bibliography_headings);

  let input_node_count: usize = document.hir().source_nodes().iter().map(Vec::len).sum::<usize>()
    + document.bibliography().map_or(0, <[BibliographyEntry]>::len);
  debug!(input_node_count, layout_node_count = result.len(), "LayoutNode へ lowering");
  return (result, headings);
}

/// HIR のブロック列を変換し、水平アキを後続段落の先頭へまとめる。
///
/// 水平アキの後に段落が来なければ、ブロック境界または列の末尾でアキだけの段落を閉じる。
pub(super) fn lower_nodes<'a>(
  ctx: &LoweringContext<'_>,
  nodes: impl IntoIterator<Item = &'a HirNode>,
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  let mut pending_space = Vec::new();
  // アキだけで閉じる段落が指す、アキ列の先頭の `Space` ノード
  let mut pending_space_origin = None;
  for node in nodes {
    let lowered = match &node.kind {
      HirNodeKind::Heading(heading) => heading::lower_hir_heading(ctx, node.id, heading, state),
      HirNodeKind::Paragraph(inlines) => {
        let mut paragraph = paragraph::lower_paragraph(ctx, node.id, inlines, state);
        if !pending_space.is_empty() {
          pending_space.append(&mut paragraph.content);
          paragraph.content = mem::take(&mut pending_space);
        }
        vec![LayoutNode::Paragraph(paragraph)]
      },
      HirNodeKind::List(list) => list::lower_list(ctx, list, state),
      HirNodeKind::Theorem(theorem) => theorem::lower_theorem(ctx, node.id, theorem, state),
      HirNodeKind::Quote(quote) => quote::lower_quote(ctx, quote, state),
      HirNodeKind::Flush(flush) => flush::lower_flush(ctx, flush, state),
      HirNodeKind::CodeBlock(text) => code::lower_code_block(ctx, node.id, text),
      HirNodeKind::PageBreak => vec![LayoutNode::PageBreak],
      HirNodeKind::Space(length) => {
        pending_space.push(InlineNode::Kern { length: *length });
        pending_space_origin.get_or_insert(node.id);
        continue;
      },
      HirNodeKind::MathBlock(math) => math::lower_math_block(ctx, node.id, math, &*state),
      HirNodeKind::Figure(figure) => figure::lower_figure(ctx, node.id, figure, state),
      HirNodeKind::Table(table) => table::lower_table(ctx, node.id, table, state),
    };
    if !lowered.is_empty() && !pending_space.is_empty() {
      result.push(LayoutNode::Paragraph(ParagraphLayout {
        origin: pending_space_origin,
        content: mem::take(&mut pending_space),
        spacing_after: Length::ZERO,
      }));
    }
    if !lowered.is_empty() {
      pending_space_origin = None;
    }
    result.extend(lowered);
  }
  if !pending_space.is_empty() {
    result.push(LayoutNode::Paragraph(ParagraphLayout {
      origin: pending_space_origin,
      content: pending_space,
      spacing_after: Length::ZERO,
    }));
  }
  return result;
}

/// ラベル付きブロック（図・表・定理・ディスプレイ数式）の先頭に `\ref` 到達先アンカーを付与する
///
/// ディスプレイ数式の行ラベルも、複数行がラベルを持つ場合を含めすべてブロック先頭座標に解決される。
fn with_label_anchors<'a>(labels: impl IntoIterator<Item = &'a LabelId>, nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let mut result: Vec<LayoutNode> =
    labels.into_iter().map(|label| return LayoutNode::Anchor(AnchorId::Label(label.clone()))).collect();
  if result.is_empty() {
    return nodes;
  }
  result.extend(nodes);
  return result;
}

#[cfg(test)]
mod tests {
  use super::{
    test_support::{analyzed, context, inlines, paragraph_layout, paragraph_origins},
    *,
  };
  use crate::{
    document::{HirDocument, HirInlineKind},
    frontend::test_support::parse_for_test,
    semantics::{SemanticDocument, SemanticPolicy, analyze_for_test, test_support::sample_references},
    source::SourceId,
    typeset::boxes::{AnchorId, LinkTarget},
  };

  /// 複数の `.sei` ソースを 1 つの文書として parse → analyze するテストヘルパ
  fn analyzed_sources(sources: &[&str]) -> SemanticDocument {
    let hir = HirDocument::assemble(
      sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
          return parse_for_test(source, SourceId::new(index)).expect("パースに成功するはず");
        })
        .collect(),
    );
    return analyze_for_test(hir, &SemanticPolicy::from_style(&ReadStyle::default()), &sample_references())
      .expect("解析できる入力のはず");
  }

  /// 入力を lower して、レイアウトノード列と見出し記録の両方を返すテストヘルパ
  fn lower_body(style: &ReadStyle, document: &SemanticDocument) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
    let ctx = context(style);
    return lower_sources_with_headings(&ctx, document);
  }

  /// `.sei` ソース 1 本を lower して `LayoutNode` 列だけを返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> {
    return test_support::lower(style, &analyzed(source));
  }

  /// レイアウトノード木を再帰的に走査し、`LineBreak` が含まれるか調べるヘルパ
  fn contains_line_break(nodes: &[LayoutNode]) -> bool {
    return nodes.iter().any(|n| match n {
      LayoutNode::Paragraph(paragraph) => return contains_line_break_inline(&paragraph.content),
      LayoutNode::VBox { children, .. } => {
        return contains_line_break(children);
      },
      LayoutNode::Table(table) => {
        return table
          .head
          .iter()
          .chain(table.rows.iter())
          .any(|row| return row.cells.iter().any(|cell| return contains_line_break_inline(&cell.content)));
      },
      _ => return false,
    });
  }

  /// [`contains_line_break`] のインライン列側（`Raise` / `Scripts` の中身は `AtomNode` で `LineBreak` を持てない）
  fn contains_line_break_inline(nodes: &[InlineNode]) -> bool {
    return nodes.iter().any(|n| match n {
      InlineNode::LineBreak => return true,
      _ => return false,
    });
  }

  #[test]
  fn lower_space_becomes_horizontal_kern() {
    let style = ReadStyle::default();

    let result = lower_source(&style, r"\space{5pt}");

    let kern = inlines(&result)
      .find_map(|n| match n {
        InlineNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .expect("Kern が出力されるはず");
    assert!((kern.to_pt() - 5.0).abs() < f32::EPSILON, "{result:?}");
    assert_eq!(paragraph_layout(&result[0]).spacing_after, Length::ZERO);
  }

  #[test]
  fn horizontal_spaces_precede_the_indent_of_the_following_paragraph() {
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(10.0);

    let nodes = lower_source(&style, r"\space{5pt}\space{-2pt}body");

    assert_eq!(nodes.len(), 1);
    let paragraph = paragraph_layout(&nodes[0]);
    let [
      InlineNode::Kern { length: first },
      InlineNode::Kern { length: second },
      InlineNode::Kern { length: indent },
      InlineNode::Text(text, _),
    ] = paragraph.content.as_slice()
    else {
      panic!("水平アキ列・字下げ・本文の順に 1 段落へ入るはず: {nodes:?}");
    };
    assert_eq!((*first, *second, *indent), (Length::pt(5.0), Length::pt(-2.0), Length::pt(10.0)));
    assert_eq!(text, "body");
    assert_eq!(paragraph.spacing_after, style.text.paragraph_spacing);
  }

  #[test]
  fn horizontal_space_continues_across_sources_and_empty_lists() {
    let style = ReadStyle::default();
    let document = analyzed_sources(&[r"\space{5pt}", r"\begin{itemize}\end{itemize}body"]);

    let (nodes, _) = lower_body(&style, &document);

    assert_eq!(nodes.len(), 1);
    assert!(matches!(
      paragraph_layout(&nodes[0]).content.as_slice(),
      [InlineNode::Kern { length }, InlineNode::Text(text, _)] if *length == Length::pt(5.0) && text == "body"
    ));
  }

  #[test]
  fn horizontal_space_closes_before_a_vertical_block() {
    let style = ReadStyle::default();

    let nodes = lower_source(&style, r"\space{5pt}\pagebreak body");

    assert!(matches!(
      nodes.as_slice(),
      [
        LayoutNode::Paragraph(_),
        LayoutNode::PageBreak,
        LayoutNode::Paragraph(_)
      ]
    ));
    let paragraph = paragraph_layout(&nodes[0]);
    assert!(matches!(paragraph.content.as_slice(), [InlineNode::Kern { length }] if *length == Length::pt(5.0)));
    assert_eq!(paragraph.spacing_after, Length::ZERO);
  }

  #[test]
  fn zero_spacing_keeps_adjacent_paragraphs_separate() {
    let mut style = ReadStyle::default();
    style.text.paragraph_spacing = Length::ZERO;

    let nodes = lower_source(&style, "first\n\nsecond");

    assert_eq!(nodes.len(), 2);
    for (node, expected) in nodes.iter().zip(["first", "second"]) {
      let paragraph = paragraph_layout(node);
      assert!(matches!(paragraph.content.as_slice(), [InlineNode::Text(text, _)] if text == expected));
      assert_eq!(paragraph.spacing_after, Length::ZERO);
    }
  }

  #[test]
  fn lower_page_break() {
    let style = ReadStyle::default();

    let result = lower_source(&style, "\\pagebreak\n");

    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn lower_math_block_wraps_with_vkerns_and_emits_no_line_break() {
    let style = ReadStyle::default();

    let out = lower_source(&style, "\\begin{equation}[numbered=false]\na\n\\end{equation}\n");

    assert_eq!(out.len(), 3, "Vkern + MathBlock + Vkern の 3 要素: {out:?}");
    assert!(matches!(out.first(), Some(LayoutNode::Vkern { .. })), "先頭は Vkern であるべき: {out:?}");
    assert!(matches!(out.get(1), Some(LayoutNode::MathBlock(_))), "中央は MathBlock であるべき: {out:?}");
    assert!(matches!(out.last(), Some(LayoutNode::Vkern { .. })), "末尾は Vkern であるべき: {out:?}");
    assert!(!inlines(&out).any(|n| matches!(n, InlineNode::LineBreak)), "LineBreak は出力されないはず: {out:?}");
  }

  #[test]
  fn lower_nodes_dispatches_each_variant_in_order() {
    let style = ReadStyle::default();

    let out = lower_source(&style, "\\section{H}\n\nP\n\n\\begin{itemize}\n\\item{L}\n\\end{itemize}\n\n\\pagebreak\n");

    let vbox_count = out.iter().filter(|n| matches!(n, LayoutNode::VBox { .. })).count();
    assert!(vbox_count >= 2, "見出しとリスト項目で VBox が複数出る: {out:?}");
    assert!(inlines(&out).any(|n| matches!(n, InlineNode::Text(t, _) if t == "P")), "段落 Text が出る: {out:?}");
    assert!(matches!(out.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {out:?}");
  }

  #[test]
  fn nested_heading_gets_sequential_anchor_index() {
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{Top1}\n\n\\begin{quote}\n\\section{Nested}\n\\end{quote}\n\n\\section{Top2}\n");

    let (layout, headings) = lower_body(&style, &analyzed);

    assert_eq!(headings.len(), 3, "見出しは 3 件記録されるはず: {headings:?}");
    let indices: Vec<usize> = headings.iter().map(|h| return h.index).collect();
    assert_eq!(indices, vec![0, 1, 2], "見出し index は文書順に連番のはず: {headings:?}");
    // 左辺（アンカー）は「レイアウト木を文書順に辿って現れた順」、右辺（見出し記録）は
    // 「`analyze` が facts に積んだ順」で、出所が独立している。集合一致では key の入れ替わりを
    // 検出できないため、ソートせず順序も含めて比較する。
    let anchor_keys = collect_heading_anchor_keys(&layout);
    assert_eq!(anchor_keys, indices, "アンカーの key は見出し記録の index と順序込みで一致するはず: {layout:?}");
  }

  /// レイアウトノード木から `AnchorId::Heading` の key（文書順インデックス）を集める
  fn collect_heading_anchor_keys(nodes: &[LayoutNode]) -> Vec<usize> {
    let mut keys = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Anchor(AnchorId::Heading(key)) => keys.push(key.index()),
        LayoutNode::VBox { children, .. } => {
          keys.extend(collect_heading_anchor_keys(children));
        },
        _ => {},
      }
    }
    return keys;
  }

  #[test]
  fn block_boundaries_use_no_bare_line_break() {
    let style = ReadStyle::default();

    let out =
      lower_source(&style, "\\section{Heading}\n\nPara\n\n\\begin{enumerate}\n\\item{Item}\n\\end{enumerate}\n");

    assert!(!contains_line_break(&out), "段落内 \\\\ 以外で LineBreak は出力されない: {out:?}");
  }

  #[test]
  fn footnote_indices_continue_across_sources() {
    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["one \\footnote{a}\n", "two \\footnote{b}\n"]);

    let (layout, _headings) = lower_body(&style, &analyzed);

    let indices: Vec<u32> = inlines(&layout)
      .filter_map(|n| match n {
        InlineNode::Footnote { index, .. } => return Some(*index),
        _ => return None,
      })
      .collect();
    assert_eq!(indices, vec![0, 1], "脚注の出現 index はソースを跨いで通し番号: {layout:?}");
  }

  #[test]
  fn labeled_display_math_emits_label_anchor() {
    let style = ReadStyle::default();

    let out = lower_source(&style, "\\begin{equation}[label=eq:foo]\na\n\\end{equation}\n");

    assert!(
      matches!(out.first(), Some(LayoutNode::Anchor(AnchorId::Label(l))) if l.as_str() == "eq:foo"),
      "先頭は Label アンカー: {out:?}"
    );
  }

  #[test]
  fn unlabeled_display_math_emits_no_anchor() {
    let style = ReadStyle::default();

    let out = lower_source(&style, "\\begin{equation}\na\n\\end{equation}\n");

    assert!(!out.iter().any(|n| matches!(n, LayoutNode::Anchor(_))), "アンカーは出ない: {out:?}");
  }

  #[test]
  fn display_math_row_label_anchors_are_reversed() {
    let style = ReadStyle::default();

    let out =
      lower_source(&style, "\\begin{align}\na &= b \\label{eq:first} \\\\\nc &= d \\label{eq:second}\n\\end{align}\n");

    let anchors: Vec<&str> = out
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Anchor(AnchorId::Label(label)) => return Some(label.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(anchors, vec!["eq:second", "eq:first"], "{out:?}");
    assert!(
      matches!(out.get(2), Some(LayoutNode::Vkern { .. })),
      "アンカー 2 個の直後からブロック本体が始まる: {out:?}"
    );
  }

  #[test]
  fn default_font_size_reflects_core_font_size() {
    let mut style = ReadStyle::default();
    style.text.font_size = Length::pt(18.0);

    let out = lower_source(&style, "x\n");

    let InlineNode::Text(_, text_style) = &paragraph_layout(&out[0]).content[0] else {
      panic!("先頭は Text であるべき: {out:?}");
    };
    assert_eq!(text_style.font_size, Length::pt(18.0));
  }

  #[test]
  fn numbering_continues_across_sources() {
    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["\\chapter{A}\n", "\\chapter{B}\n"]);

    let (_layout, headings) = lower_body(&style, &analyzed);

    assert_eq!(headings.len(), 2, "{headings:?}");
    assert_eq!(headings[0].number, "1", "1 ソース目の chapter は 1");
    assert_eq!(headings[1].number, "2", "2 ソース目の chapter は連番の 2: {headings:?}");
  }

  #[test]
  fn ref_resolves_across_sources() {
    fn contains_internal_link(nodes: &[LayoutNode], target: &str) -> bool {
      return nodes.iter().any(|n| match n {
        LayoutNode::Paragraph(paragraph) => return contains_internal_link_inline(&paragraph.content, target),
        LayoutNode::VBox { children, .. } => {
          return contains_internal_link(children, target);
        },
        _ => return false,
      });
    }

    /// [`contains_internal_link`] のインライン列側
    fn contains_internal_link_inline(nodes: &[InlineNode], target: &str) -> bool {
      return nodes.iter().any(|n| match n {
        InlineNode::Link {
          target: LinkTarget::Internal(t),
          ..
        } => return *t == AnchorId::Label(LabelId::new(target)),
        _ => return false,
      });
    }

    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["\\chapter[label=ch:intro]{Intro}\n", "\\ref{ch:intro}\n"]);

    let (layout, _headings) = lower_body(&style, &analyzed);

    assert!(contains_internal_link(&layout, "ch:intro"), "跨りの \\ref が解決されるはず: {layout:?}");
  }

  #[test]
  fn heading_number_uses_style_number_format() {
    let style = ReadStyle::default();
    let analyzed = analyzed("\\chapter{C}\n\n\\section{S}\n\n\\section{S2}\n");

    let (_layout, headings) = lower_body(&style, &analyzed);

    let numbers: Vec<&str> = headings.iter().map(|h| return h.number.as_str()).collect();
    assert_eq!(numbers, vec!["1", "1.1", "1.2"], "section は既定で \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn heading_title_plain_resolves_embedded_ref() {
    let style = ReadStyle::default();
    let analyzed = analyzed("\\chapter[label=ch:intro]{Intro}\n\n\\section{見出し \\ref{ch:intro}}\n");

    let (_layout, headings) = lower_body(&style, &analyzed);

    assert_eq!(headings[1].title_plain, "見出し Chapter 1", "タイトル中の \\ref も表示文字列になる");
  }

  #[test]
  fn heading_title_plain_uses_generated_citation_display() {
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{結論 \\cite{kwan2014}}\n");
    let site = analyzed.citation_sites().next().expect("引用箇所が 1 件あるはず");
    let document =
      analyzed.with_generated_citations_for_test(vec![(site, vec![GeneratedInline::Text("[1]".to_string())])], None);
    let ctx = context(&style);

    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

    assert_eq!(headings[0].title_plain, "結論 [1]", "{headings:?}");
  }

  #[test]
  fn heading_label_combines_number_and_title() {
    let record = |number: &str, title_plain: &str| {
      return HeadingRecord {
        node: None,
        index: 0,
        level: HeadingLevel::Section,
        number: number.to_string(),
        title_plain: title_plain.to_string(),
      };
    };
    assert_eq!(record("1.2", "Intro").label(), "1.2 Intro");
    assert_eq!(record("", "Intro").label(), "Intro");
    assert_eq!(record("1.2", "").label(), "1.2");
  }

  /// 1 本目のソースのトップレベル HIR ノードの ID を文書順に返す
  fn top_level_ids(document: &SemanticDocument) -> Vec<NodeId> {
    return document.hir().source_nodes()[0].iter().map(|node| return node.id).collect();
  }

  #[test]
  fn paragraph_carries_its_hir_node_id() {
    let document = analyzed("一段落目。\n\n二段落目。\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let ids = top_level_ids(&document);
    assert_eq!(paragraph_origins(&nodes), vec![Some(ids[0]), Some(ids[1])]);
  }

  #[test]
  fn pending_space_merged_into_paragraph_keeps_paragraph_origin() {
    // 段落へ合流したアキはその段落を指し、ブロックの前で閉じたアキだけの段落は先頭の Space を指す
    let document = analyzed(r"\space{5pt}body\space{3pt}\space{4pt}\pagebreak tail");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let hir = &document.hir().source_nodes()[0];
    let id_of = |pick: fn(&HirNodeKind) -> bool, nth: usize| {
      return hir.iter().filter(|node| return pick(&node.kind)).nth(nth).expect("ノードがあるはず").id;
    };
    let is_paragraph: fn(&HirNodeKind) -> bool = |kind| return matches!(kind, HirNodeKind::Paragraph(_));
    let is_space: fn(&HirNodeKind) -> bool = |kind| return matches!(kind, HirNodeKind::Space(_));
    assert_eq!(
      paragraph_origins(&nodes),
      vec![
        Some(id_of(is_paragraph, 0)),
        Some(id_of(is_space, 1)),
        Some(id_of(is_paragraph, 1))
      ]
    );
  }

  #[test]
  fn code_block_paragraph_points_at_the_code_environment() {
    let document = analyzed("\\begin{code}\nfn main() {}\n\\end{code}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    assert_eq!(paragraph_origins(&nodes), vec![Some(top_level_ids(&document)[0])]);
  }

  #[test]
  fn heading_paragraph_points_at_the_heading() {
    let document = analyzed("\\section{見出し}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    assert_eq!(paragraph_origins(&nodes), vec![Some(top_level_ids(&document)[0])]);
  }

  #[test]
  fn theorem_heading_points_at_the_theorem() {
    // 定理見出しの段落は定理環境を指し、本文の段落は自身を指す
    let document = analyzed("\\begin{theorem}\n本文。\n\\end{theorem}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let theorem = &document.hir().source_nodes()[0][0];
    let HirNodeKind::Theorem(body) = &theorem.kind else {
      panic!("定理が期待されます: {theorem:?}");
    };
    assert_eq!(paragraph_origins(&nodes), vec![Some(theorem.id), Some(body.body[0].id)]);
  }

  #[test]
  fn caption_paragraph_points_at_the_float() {
    let document = analyzed("\\begin{figure}\n\\image{a.png}\n\\caption{説明}\n\\end{figure}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    assert_eq!(paragraph_origins(&nodes), vec![Some(top_level_ids(&document)[0])]);
  }

  #[test]
  fn list_marker_only_paragraph_points_at_the_item() {
    // 本体が段落で始まらない項目は、マーカーだけの段落を項目自身に帰属させる
    let document = analyzed("\\begin{itemize}\n\\item{\\begin{code}\nx\n\\end{code}}\n\\end{itemize}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let list = &document.hir().source_nodes()[0][0];
    let HirNodeKind::List(list) = &list.kind else {
      panic!("リストが期待されます: {list:?}");
    };
    let item = &list.items[0];
    assert_eq!(paragraph_origins(&nodes), vec![Some(item.id), Some(item.body[0].id)]);
  }

  /// 最初の画像ノードの起源を返す（`VBox` の中へも降りる）
  fn image_origin(nodes: &[LayoutNode]) -> Option<NodeId> {
    return nodes.iter().find_map(|node| match node {
      LayoutNode::Image { origin, .. } => return Some(*origin),
      LayoutNode::VBox { children, .. } => return image_origin(children),
      _ => return None,
    });
  }

  #[test]
  fn figure_image_points_at_the_image_command() {
    let document = analyzed("\\begin{figure}\n\\image{a.png}\n\\end{figure}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let figure = &document.hir().source_nodes()[0][0];
    let HirNodeKind::Figure(figure) = &figure.kind else {
      panic!("図が期待されます: {figure:?}");
    };
    assert_eq!(image_origin(&nodes), Some(figure.image_site));
  }

  #[test]
  fn math_block_points_at_the_environment() {
    let document = analyzed("\\begin{equation}\nx\n\\end{equation}\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let origin = nodes.iter().find_map(|node| match node {
      LayoutNode::MathBlock(block) => return Some(block.origin),
      _ => return None,
    });
    assert_eq!(origin, Some(top_level_ids(&document)[0]));
  }

  #[test]
  fn footnote_points_at_the_footnote_command() {
    let document = analyzed("本文\\footnote{注}。\n");

    let nodes = test_support::lower(&ReadStyle::default(), &document);

    let paragraph = &document.hir().source_nodes()[0][0];
    let HirNodeKind::Paragraph(hir_inlines) = &paragraph.kind else {
      panic!("段落が期待されます: {paragraph:?}");
    };
    let footnote_id = hir_inlines
      .iter()
      .find(|inline| return matches!(inline.kind, HirInlineKind::Footnote { .. }))
      .expect("脚注があるはず")
      .id;
    let origin = inlines(&nodes).find_map(|node| match node {
      InlineNode::Footnote { origin, .. } => return Some(*origin),
      _ => return None,
    });
    assert_eq!(origin, Some(footnote_id));
  }

  #[test]
  fn heading_records_carry_heading_node() {
    let document = analyzed("\\section{見出し}\n");

    let (_, headings) = lower_body(&ReadStyle::default(), &document);

    assert_eq!(
      headings.iter().map(|heading| return heading.node).collect::<Vec<_>>(),
      vec![Some(top_level_ids(&document)[0])]
    );
  }
}
