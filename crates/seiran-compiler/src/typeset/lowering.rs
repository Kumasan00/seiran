//! Lowering 層: 意味解析の成果物（`semantics::SemanticDocument`）→ `LayoutNode` 変換
//!
//! ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` が済ませているため、
//! この層は「確定した構造値を style の表示側フィールドで文字列にして箱に積む」だけを行う。
//!
//! 著者が書いた本文は HIR（`document::hir`）を走査し、事実は `NodeId` をキーに [`LoweringState`] の
//! query で引く。CSL 整形の生成物（書誌・引用表示）は `NodeId` を持たないので、別経路
//! （子 module `generated`）で lower する。

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

pub(super) use layout_node::{
  AtomNode, DelimiterGlyphs, InlineNode, LayoutNode, MathBlockLayout, MathFraction, MathScripts, TableLayout,
  TableRowLayout, TextStyle,
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
  use super::{InlineNode, LayoutNode, LoweringContext, TextStyle, lower_sources_with_headings};
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

  /// レイアウトノードがインラインなら中身を借りる（縦リスト用ノードなら `None`）
  pub(super) fn as_inline(node: &LayoutNode) -> Option<&InlineNode> {
    return match node {
      LayoutNode::Inline(inline) => Some(inline),
      _ => None,
    };
  }

  /// レイアウトノードがインラインの `Text` なら、その文字列とスタイルを借りる
  pub(super) fn inline_text(node: &LayoutNode) -> Option<(&str, TextStyle)> {
    return match as_inline(node)? {
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
  let mut result = Vec::new();
  for nodes in document.hir().source_nodes() {
    result.extend(lower_nodes(ctx, nodes, &mut state));
  }

  // 書誌は本文の後ろに置き、見出しキーは本文の見出し数の続きから振る。
  let (bibliography_nodes, bibliography_headings) =
    generated::lower_bibliography(ctx, document.bibliography(), document.heading_count());
  result.extend(bibliography_nodes);

  // 見出し一覧は facts の順（= `analyze` が振った `HeadingKey` の順）で組む。走査順に依存しない。
  let mut headings: Vec<HeadingRecord> = document
    .headings()
    .map(|heading| {
      return HeadingRecord {
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

/// `nodes` を順に [`lower_node`] で変換し、結果を出現順に連結する
pub(super) fn lower_nodes(
  ctx: &LoweringContext<'_>,
  nodes: &[HirNode],
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node(ctx, node, state));
  }
  return result;
}

/// 単一の `HirNode` をレイアウトノードに変換する
fn lower_node(ctx: &LoweringContext<'_>, node: &HirNode, state: &mut LoweringState<'_>) -> Vec<LayoutNode> {
  match &node.kind {
    HirNodeKind::Heading(heading) => {
      return heading::lower_hir_heading(ctx, node.id, heading, state);
    },
    HirNodeKind::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines, state);
    },
    HirNodeKind::List(list) => {
      return list::lower_list(ctx, list, state);
    },
    HirNodeKind::Theorem(theorem) => {
      return theorem::lower_theorem(ctx, node.id, theorem, state);
    },
    HirNodeKind::Quote(quote) => {
      return quote::lower_quote(ctx, quote, state);
    },
    HirNodeKind::Flush(flush) => {
      return flush::lower_flush(ctx, flush, state);
    },
    HirNodeKind::CodeBlock(text) => {
      return code::lower_code_block(ctx, text);
    },
    HirNodeKind::PageBreak => {
      return vec![LayoutNode::PageBreak];
    },
    HirNodeKind::Space(length) => {
      return vec![LayoutNode::Inline(InlineNode::Kern { length: *length })];
    },
    HirNodeKind::MathBlock(math) => {
      return math::lower_math_block(ctx, node.id, math, &*state);
    },
    HirNodeKind::Figure(figure) => {
      return figure::lower_figure(ctx, node.id, figure, state);
    },
    HirNodeKind::Table(table) => {
      return table::lower_table(ctx, node.id, table, state);
    },
  }
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
  use std::slice;

  use super::{
    test_support::{analyzed, context},
    *,
  };
  use crate::{
    document::HirDocument,
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
      LayoutNode::Inline(inline) => return contains_line_break_inline(slice::from_ref(inline)),
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

    let kern = result
      .iter()
      .find_map(|n| match n {
        LayoutNode::Inline(InlineNode::Kern { length }) => return Some(*length),
        _ => return None,
      })
      .expect("Kern が出力されるはず");
    assert!((kern.to_pt() - 5.0).abs() < f32::EPSILON, "{result:?}");
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
    assert!(
      !out.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::LineBreak))),
      "LineBreak は出力されないはず: {out:?}"
    );
  }

  #[test]
  fn lower_nodes_dispatches_each_variant_in_order() {
    let style = ReadStyle::default();

    let out = lower_source(&style, "\\section{H}\n\nP\n\n\\begin{itemize}\n\\item{L}\n\\end{itemize}\n\n\\pagebreak\n");

    let vbox_count = out.iter().filter(|n| matches!(n, LayoutNode::VBox { .. })).count();
    assert!(vbox_count >= 2, "見出しとリスト項目で VBox が複数出る: {out:?}");
    assert!(
      out.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Text(t, _)) if t == "P")),
      "段落 Text が出る: {out:?}"
    );
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

    let indices: Vec<u32> = layout
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Inline(InlineNode::Footnote { index, .. }) => return Some(*index),
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

    let LayoutNode::Inline(InlineNode::Text(_, text_style)) = &out[0] else {
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
        LayoutNode::Inline(inline) => return contains_internal_link_inline(slice::from_ref(inline), target),
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
}
