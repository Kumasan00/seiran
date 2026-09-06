//! Lowering 層: 意味解析の成果物（`semantics::SemanticDocument`）→ `LayoutNode` 変換
//!
//! ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` が済ませているため、
//! この層は「確定した構造値を style の表示側フィールドで文字列にして箱に積む」だけを行う。
//! 意味解析を行わないので、この層に失敗はない（`Result` を返さない）。
//!
//! 著者が書いた本文は HIR（`document::hir`）を走査し、事実は `NodeId` をキーに [`LoweringState`] の
//! query で引く。CSL 整形の生成物（書誌・引用表示）は `NodeId` を持たないので、別経路
//! （子 module `generated`）で lower する。

use tracing::debug;

use crate::{
  document::{HirInline, HirInlineKind, HirNode, HirNodeKind, NodeId, NodeMap},
  length::Length,
  semantics::{CounterValue, GeneratedInline, HeadingKey, LabelId, SemanticDocument, generated_inlines_to_plain_text},
  style::Style as ReadStyle,
  typeset::boxes::AnchorMark,
};

mod code;
mod counter;
mod figure;
mod float;
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

pub(super) use layout_node::{AtomNode, LayoutNode, MathBlockRow, TableLayout, TableRowLayout, TextStyle};
pub(crate) use title_page::{TitlePageMetadata, lower_title_page};

use crate::document::{FontKind, HeadingLevel};

/// Lowering のコンテキスト
pub(super) struct LoweringContext<'a> {
  /// スタイル設定への参照（`config/style.toml` 由来。未指定キーは `serde(default)` の既定値）
  pub style: &'a ReadStyle,
  /// 本文段落の既定フォント種別
  pub body_font_kind: FontKind,
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
}

impl<'a> LoweringContext<'a> {
  /// 新しい `LoweringContext` を生成する
  #[must_use]
  pub(super) fn new(style: &'a ReadStyle) -> Self {
    return LoweringContext {
      style,
      body_font_kind: style.text.font_kind,
      first_line_indent: style.text.first_line_indent,
      image_max_dpi: 300,
      image_downsample: true,
      list_depth: 0,
      footnote_numbers: None,
    };
  }

  /// 画像出力の既定値（config `[image]` 由来）を差し替えた文脈を返す
  #[must_use]
  pub(super) fn with_image_defaults(mut self, image_max_dpi: u32, image_downsample: bool) -> Self {
    self.image_max_dpi = image_max_dpi;
    self.image_downsample = image_downsample;
    return self;
  }

  /// 脚注の表示番号の上書きマップを与えた文脈を返す
  #[must_use]
  pub(super) fn with_footnote_numbers(mut self, numbers: &'a [u32]) -> Self {
    self.footnote_numbers = Some(numbers);
    return self;
  }

  /// 本文段落の既定フォント種別だけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_body_font_kind(&self, body_font_kind: FontKind) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind,
      first_line_indent: self.first_line_indent,
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth: self.list_depth,
      footnote_numbers: self.footnote_numbers,
    };
  }

  /// 段落先頭行の字下げ量だけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_first_line_indent(&self, first_line_indent: Length) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind: self.body_font_kind,
      first_line_indent,
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth: self.list_depth,
      footnote_numbers: self.footnote_numbers,
    };
  }

  /// 箇条書きのネスト深さだけを差し替えた派生文脈を返す
  #[must_use]
  pub(super) fn with_list_depth(&self, list_depth: usize) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind: self.body_font_kind,
      first_line_indent: self.first_line_indent,
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth,
      footnote_numbers: self.footnote_numbers,
    };
  }

  /// 既定フォントサイズ（段落本文用、`style.text.font_size` に等しい）を pt 値で返すヘルパー
  #[must_use]
  pub(super) fn default_font_size(&self) -> Length { return self.style.text.font_size; }
}

/// 見出し 1 件の記録（PDF しおり・目次生成が消費する）
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

/// 子 module のテストが lowering の入力を組み立てるための最小ヘルパ
#[cfg(test)]
pub(super) mod test_support {
  use super::{LayoutNode, LoweringContext, lower_sources_with_headings};
  use crate::{
    document::HirDocument,
    frontend::test_support::parse_source_for_test,
    semantics::{SemanticDocument, SemanticPolicy, analyze_for_test, test_fixtures::sample_references},
    source::SourceId,
    style::Style,
  };

  /// `.sei` スニペットを parse → analyze して意味解析済みドキュメントを作る
  ///
  /// `SemanticDocument` は `analyze` からしか作れない（`NodeId` を捏造できない）ので、lowering の
  /// テストは本番と同じ経路を通す。`\cite` を含むスニペットのために、参照定義には文献フィクスチャ
  /// （`kwan2014` / `doe2020`）を渡しておく。引用の表示・書誌を要るテストは
  /// `SemanticDocument::with_citations_for_test` で差し込む。
  pub(crate) fn analyzed(source: &str) -> SemanticDocument {
    let hir =
      HirDocument::assemble(vec![parse_source_for_test(source, SourceId::new(0)).expect("パースに成功するはず")]);
    return analyze_for_test(hir, &SemanticPolicy::from_style(&Style::default()), &sample_references())
      .expect("解析できる入力のはず");
  }

  /// 意味解析済みドキュメントを lower してレイアウトノード列を返す
  pub(crate) fn lower(style: &Style, document: &SemanticDocument) -> Vec<LayoutNode> {
    let ctx = LoweringContext::new(style);
    let (layout, _headings) = lower_sources_with_headings(&ctx, document);
    return layout;
  }
}

/// 走査中に更新される可変状態と、事実を引く query の窓口
///
/// 採番・`\ref` の解決・見出しキーの付与はいずれも `semantics::analyze` が済ませているため、
/// ここに残る可変状態は「脚注の出現順に払い出す通し index」と「見出しタイトルのプレーンテキスト」
/// だけになる（後者は走査中にしか作れず、[`HeadingRecord`] の組み立てで使う）。
pub(super) struct LoweringState<'a> {
  /// 意味解析の成果物（HIR + 事実 + CSL 生成物）
  document: &'a SemanticDocument,
  /// これまでに払い出した脚注の個数（次の脚注の出現 index になる）
  footnote_count: u32,
  /// 見出しノード → タイトルのプレーンテキスト（`HeadingRecord` の組み立てに使う）
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
  ///
  /// 表示の欠落を検出するのは `GeneratedCitations` の責務（完全性の不変条件はそちらが持つ）。
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
  pub(super) fn reference_target(&self, site: NodeId) -> &'a LabelId { return self.document.reference_target(site); }

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

/// 意味解析の成果物をレイアウトノードに変換し、見出し記録（PDF しおり・目次生成用）も返す
#[must_use]
pub(super) fn lower_sources_with_headings(
  ctx: &LoweringContext<'_>,
  document: &SemanticDocument,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let mut state = LoweringState::new(document);
  let mut result = Vec::new();
  // グループの起源（`HirGroup::source_id`）はエラー帰属のための情報で、`analyze` が
  // 診断を出し終えた後の lowering では読む先が無い（診断を出さないので文脈に持たない）。
  for group in document.hir().groups() {
    result.extend(lower_nodes_inner(ctx, &group.nodes, &mut state));
  }

  // 書誌は本文の後ろに置き、見出しキーは本文の見出し数の続きから振る。
  let (bibliography_nodes, bibliography_headings) =
    generated::lower_bibliography(ctx, document.bibliography(), document.headings().len());
  result.extend(bibliography_nodes);

  // 見出し一覧は facts の順（= `analyze` が振った `HeadingKey` の順）で組む。走査順に依存しない。
  let mut headings: Vec<HeadingRecord> = document
    .headings()
    .iter()
    .map(|facts| {
      return HeadingRecord {
        index: facts.key.index(),
        level: facts.level,
        number: facts
          .counter_value
          .as_ref()
          .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value)),
        title_plain: state.heading_title(facts.node).to_string(),
      };
    })
    .collect();
  headings.extend(bibliography_headings);

  let input_node_count: usize =
    document.hir().groups().iter().map(|group| return group.nodes.len()).sum::<usize>() + document.bibliography().len();
  debug!(input_node_count, layout_node_count = result.len(), "LayoutNode へ lowering");
  return (result, headings);
}

/// `nodes` を順に [`lower_node_indexed`] へ渡す内部ウォーク（本体）
pub(super) fn lower_nodes_inner(
  ctx: &LoweringContext<'_>,
  nodes: &[HirNode],
  state: &mut LoweringState<'_>,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node_indexed(ctx, node, state));
  }
  return result;
}

/// 単一の `HirNode` をレイアウトノードに変換する（事実は `node.id` で引く）
fn lower_node_indexed(ctx: &LoweringContext<'_>, node: &HirNode, state: &mut LoweringState<'_>) -> Vec<LayoutNode> {
  match &node.kind {
    HirNodeKind::Heading {
      level,
      title,
      label: _,
    } => {
      // 見出しキーは `semantics::analyze` が文書順に振ったもの。lowering は振り直さず読むだけなので、
      // 再帰（quote / theorem / list item 本体）を挟んでも `analyzed.headings()` の添字と必ず揃う。
      let key = state.heading_key(node.id);
      let label = state.declared_label(node.id).cloned();
      let number = state
        .counter_value(node.id)
        .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value));
      // プレーンテキスト（しおり・目次表示）は不変借用でしか作れないので、可変借用が要る
      // タイトルの lowering より先に済ませる。
      let plain = hir_inlines_to_plain_text(title, ctx.style, &*state);
      state.record_heading_title(node.id, plain);
      let title_style = heading::title_style(ctx, *level);
      // タイトルの lowering はクロージャで遅延させる。`heading.format` が `{title}` を含まない
      // なら一度も呼ばれず、タイトル中の `\footnote` が通し index だけ消費して消える事故を防ぐ。
      return heading::lower_heading(
        ctx,
        *level,
        &number,
        || return inline::lower_inlines(ctx, title, title_style, state),
        label,
        key,
      );
    },
    HirNodeKind::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines, state);
    },
    HirNodeKind::List {
      ordered,
      items,
      start,
      item_gap,
    } => {
      return list::lower_list(ctx, *ordered, items, *start, *item_gap, state);
    },
    HirNodeKind::Theorem {
      class,
      title,
      body,
      of,
      label: _,
    } => {
      let number = state.counter_value(node.id).map(|value| return counter::format_counter_value(ctx.style, value));
      let of_target = of.as_ref().map(|target| return state.reference_target(target.id));
      let label = state.declared_label(node.id);
      return theorem::lower_theorem(ctx, *class, number.as_deref(), title.as_deref(), body, of_target, label, state);
    },
    HirNodeKind::Quote { kind, body } => {
      return quote::lower_quote(ctx, *kind, body, state);
    },
    HirNodeKind::CodeBlock { text } => {
      return code::lower_code_block(ctx, text);
    },
    HirNodeKind::PageBreak => {
      return vec![LayoutNode::PageBreak];
    },
    HirNodeKind::Space(length) => {
      return vec![LayoutNode::Kern { length: *length }];
    },
    HirNodeKind::MathBlock {
      kind,
      rows,
      numbered: _,
      label: _,
    } => {
      let block = &ctx.style.math.block;
      // ラベル付き行（`equation` の `[label=...]`、`align` / `gather` の行末 `\label{...}`）の `\ref`
      // 到達先アンカーを先頭に付ける。複数行がラベルを持つ場合も、いずれもブロック先頭座標に解決される。
      // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する。
      let mut anchor_labels: Vec<&LabelId> = Vec::new();
      if let Some(env_label) = state.declared_label(node.id) {
        anchor_labels.push(env_label);
      }
      // 行ラベルは逆順で積む（「後から prepend」を繰り返す旧実装と同じ最終順序を 1 パスで
      // 再現するため。`with_label_anchors` の doc comment も参照）
      anchor_labels.extend(rows.iter().rev().filter_map(|row| return state.declared_label(row.id)));

      let math_block = math::lower_math_block(ctx, *kind, rows, state.counter_value(node.id), &*state);
      let nodes = vec![
        LayoutNode::Vkern {
          length: block.top_margin,
        },
        math_block,
        LayoutNode::Vkern {
          length: block.bottom_margin,
        },
      ];
      return with_label_anchors(&anchor_labels, nodes);
    },
    HirNodeKind::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label: _,
    } => {
      let Some(counter_value) = state.counter_value(node.id) else {
        unreachable!("図は必ず採番される（analyze の Figure 分岐が counters へ登録している）: {:?}", node.id)
      };
      let number = counter::format_counter_value(ctx.style, counter_value);
      let label = state.declared_label(node.id);
      let caption_arg = caption.as_deref().map(|inlines| return (*caption_position, inlines));
      let overrides = figure::ImageOverrides {
        dpi: *dpi,
        downsample: *downsample,
      };
      let nodes = figure::lower_figure(ctx, image_path, *width, *height, overrides, caption_arg, &number, state);
      return with_label_anchor(label, nodes);
    },
    HirNodeKind::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      label: _,
      breakable,
    } => {
      let Some(counter_value) = state.counter_value(node.id) else {
        unreachable!("表は必ず採番される（analyze の Table 分岐が counters へ登録している）: {:?}", node.id)
      };
      let number = counter::format_counter_value(ctx.style, counter_value);
      let label = state.declared_label(node.id);
      let caption_arg = caption.as_deref().map(|inlines| return (*caption_position, inlines));
      let nodes = table::lower_table(ctx, columns, widths, head, rows, caption_arg, &number, *breakable, state);
      return with_label_anchor(label, nodes);
    },
  }
}

/// ラベル付きブロック（図・表・ディスプレイ数式）の先頭に `\ref` 到達先アンカーを付与する
fn with_label_anchor(label: Option<&LabelId>, nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let Some(label) = label else {
    return nodes;
  };
  let mut result = Vec::with_capacity(nodes.len() + 1);
  result.push(LayoutNode::Anchor(AnchorMark::Label(label.clone())));
  result.extend(nodes);
  return result;
}

/// 複数のラベルを先頭からこの順でアンカーとして 1 回の構築でまとめて付与する
fn with_label_anchors(labels: &[&LabelId], nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  if labels.is_empty() {
    return nodes;
  }
  let mut result = Vec::with_capacity(nodes.len() + labels.len());
  result.extend(labels.iter().map(|label| return LayoutNode::Anchor(AnchorMark::Label((*label).clone()))));
  result.extend(nodes);
  return result;
}

/// HIR のインライン列をプレーンテキストへ畳む（見出しタイトルのしおり・目次表示用）
///
/// 旧 `GeneratedInline` 版のプレーンテキスト畳み込みと同じ規則を保つ。バリアントごとの扱い（数式は
/// `"[Math]"`、脚注・索引は空、`\cite` は整形済み表示を辿る等）は同じに保つ。
fn hir_inlines_to_plain_text(inlines: &[HirInline], style: &ReadStyle, state: &LoweringState<'_>) -> String {
  let mut out = String::new();
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Text(s) => out.push_str(s),
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. } => {
        out.push_str(&hir_inlines_to_plain_text(children, style, state));
      },
      // 引用の表示は生成物の side table にある（見出しの `\cite` も目次・しおりでは表示を辿る）。
      // 生成物は `GeneratedInline` なので生成物側の畳み込みをそのまま使う。
      HirInlineKind::Cite { .. } => {
        out.push_str(&generated_inlines_to_plain_text(state.citation_display(inline.id)));
      },
      HirInlineKind::Code(text) => out.push_str(text),
      HirInlineKind::InlineMath(_) => out.push_str("[Math]"),
      HirInlineKind::Symbol(ch) => out.push(*ch),
      HirInlineKind::LineBreak => out.push('\n'),
      // 脚注本体・索引マーカーは見出しのプレーンテキスト抽出には含めない（NoIndent と同じ空扱い）
      HirInlineKind::NoIndent | HirInlineKind::Footnote { .. } | HirInlineKind::Index { .. } => {},
      HirInlineKind::Ref { .. } => out.push_str(&state.ref_display(style, state.reference_target(inline.id))),
    }
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::{test_support::analyzed, *};
  use crate::{
    document::HirDocument,
    frontend::test_support::parse_source_for_test,
    semantics::{SemanticDocument, SemanticPolicy, analyze_for_test, test_fixtures::sample_references},
    source::SourceId,
    style::CounterTemplate,
    typeset::boxes::{AnchorId, AnchorMark, LinkTarget},
  };

  /// 複数の `.sei` ソースを 1 つの文書として parse → analyze するテストヘルパ
  ///
  /// 採番・`\ref` 解決がソース跨ぎで通ることを見るテストだけが使う。
  fn analyzed_sources(sources: &[&str]) -> SemanticDocument {
    let hir = HirDocument::assemble(
      sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
          return parse_source_for_test(source, SourceId::new(index)).expect("パースに成功するはず");
        })
        .collect(),
    );
    return analyze_for_test(hir, &SemanticPolicy::from_style(&ReadStyle::default()), &sample_references())
      .expect("解析できる入力のはず");
  }

  /// 入力を lower して、レイアウトノード列と見出し記録の両方を返すテストヘルパ
  fn lower_body(style: &ReadStyle, document: &SemanticDocument) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
    let ctx = LoweringContext::new(style);
    return lower_sources_with_headings(&ctx, document);
  }

  /// `.sei` ソース 1 本を lower して `LayoutNode` 列だけを返すテストヘルパ
  fn lower_source(style: &ReadStyle, source: &str) -> Vec<LayoutNode> {
    return test_support::lower(style, &analyzed(source));
  }

  /// レイアウトノード木を再帰的に走査し、`LineBreak` が含まれるか調べるヘルパ
  fn contains_line_break(nodes: &[LayoutNode]) -> bool {
    return nodes.iter().any(|n| match n {
      LayoutNode::LineBreak => return true,
      // `Raise` の子は `AtomNode`（テキストと入れ子の `Raise` のみ）で、`LineBreak` を持てない
      LayoutNode::VBox { children, .. } => {
        return contains_line_break(children);
      },
      LayoutNode::Table(table) => {
        return table
          .head
          .iter()
          .chain(table.rows.iter())
          .any(|row| return row.cells.iter().any(|cell| return contains_line_break(&cell.content)));
      },
      _ => return false,
    });
  }

  #[test]
  fn lower_space_becomes_horizontal_kern() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let result = lower_source(&style, r"\space{5}");

    // Assert
    let kern = result
      .iter()
      .find_map(|n| match n {
        LayoutNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .expect("Kern が出力されるはず");
    assert!((kern.to_pt() - 5.0).abs() < f32::EPSILON, "{result:?}");
  }

  #[test]
  fn lower_page_break() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let result = lower_source(&style, "\\pagebreak\n");

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn lower_inline_math_replaces_placeholder() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "$x^{2}$\n");

    // Assert
    let placeholder = out.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "[Math]"));
    assert!(!placeholder, "[Math] プレースホルダは消えているはず: {out:?}");
    let has_raise = out.iter().any(|n| matches!(n, LayoutNode::Raise { .. }));
    assert!(has_raise, "上付き由来の Raise が含まれるはず: {out:?}");
  }

  #[test]
  fn lower_math_block_wraps_with_vkerns_and_emits_no_line_break() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "\\begin{equation}[numbered=false]\na\n\\end{equation}\n");

    // Assert
    assert_eq!(out.len(), 3, "Vkern + MathBlock + Vkern の 3 要素: {out:?}");
    assert!(matches!(out.first(), Some(LayoutNode::Vkern { .. })), "先頭は Vkern であるべき: {out:?}");
    assert!(matches!(out.get(1), Some(LayoutNode::MathBlock { .. })), "中央は MathBlock であるべき: {out:?}");
    assert!(matches!(out.last(), Some(LayoutNode::Vkern { .. })), "末尾は Vkern であるべき: {out:?}");
    assert!(!out.iter().any(|n| matches!(n, LayoutNode::LineBreak)), "LineBreak は出力されないはず: {out:?}");
  }

  #[test]
  fn lower_math_block_carries_numbered_row() {
    // Arrange
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = CounterTemplate::parse("{n}");

    // Act
    let out = lower_source(&style, "\\begin{equation}\na\n\\end{equation}\n");

    // Assert
    let Some(LayoutNode::MathBlock { rows, .. }) = out.get(1) else {
      panic!("中央に MathBlock があるべき: {out:?}");
    };
    assert_eq!(rows.len(), 1, "equation は 1 行: {rows:?}");
    let number = rows[0].number.as_ref().expect("採番された行は番号ボックスを持つ");
    assert!(matches!(&number[0], AtomNode::Text(t, _) if t == "(1)"), "番号ボックスは Text(\"(1)\"): {number:?}");
  }

  #[test]
  fn lower_nodes_dispatches_each_variant_in_order() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "\\section{H}\n\nP\n\n\\begin{itemize}\n\\item{L}\n\\end{itemize}\n\n\\pagebreak\n");

    // Assert
    let vbox_count = out.iter().filter(|n| matches!(n, LayoutNode::VBox { .. })).count();
    assert!(vbox_count >= 2, "見出しとリスト項目で VBox が複数出る: {out:?}");
    assert!(out.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "P")), "段落 Text が出る: {out:?}");
    assert!(matches!(out.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {out:?}");
  }

  #[test]
  fn nested_heading_gets_sequential_anchor_index() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{Top1}\n\n\\begin{quote}\n\\section{Nested}\n\\end{quote}\n\n\\section{Top2}\n");

    // Act
    let (layout, headings) = lower_body(&style, &analyzed);

    // Assert
    assert_eq!(headings.len(), 3, "見出しは 3 件記録されるはず: {headings:?}");
    let indices: Vec<usize> = headings.iter().map(|h| return h.index).collect();
    assert_eq!(indices, vec![0, 1, 2], "見出し index は文書順に連番のはず: {headings:?}");
    // `AnchorMark::Heading` の key が `HeadingRecord::index` と 1:1 かつ同順で対応することを確かめる。
    //
    // 左辺（アンカー）は「レイアウト木を文書順に辿って現れた順」、右辺（見出し記録）は
    // 「`analyze` が facts に積んだ順」で、出所が独立している。両者がずれると
    // `compiler::front_matter` が見出しとページ番号を zip するときに目次のページ番号が
    // 静かにずれる（長さ違いは debug_assert しか見ておらず release では素通りする）。
    // 集合一致では key の入れ替わりを検出できないため、ソートせず順序も含めて比較する。
    let anchor_keys = collect_heading_anchor_keys(&layout);
    assert_eq!(anchor_keys, indices, "アンカーの key は見出し記録の index と順序込みで一致するはず: {layout:?}");
  }

  /// レイアウトノード木から `AnchorMark::Heading` の key（文書順インデックス）を集める
  fn collect_heading_anchor_keys(nodes: &[LayoutNode]) -> Vec<usize> {
    let mut keys = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Anchor(AnchorMark::Heading { key, .. }) => keys.push(key.index()),
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
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out =
      lower_source(&style, "\\section{Heading}\n\nPara\n\n\\begin{enumerate}\n\\item{Item}\n\\end{enumerate}\n");

    // Assert
    assert!(!contains_line_break(&out), "段落内 \\\\ 以外で LineBreak は出力されない: {out:?}");
  }

  #[test]
  fn footnotes_across_paragraphs_number_sequentially() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "one \\footnote{a}\n\ntwo \\footnote{b}\n\nthree \\footnote{c}\n");

    // Assert
    let numbers: Vec<u32> = out
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { number, .. } => return Some(*number),
        _ => return None,
      })
      .collect();
    assert_eq!(numbers, vec![1, 2, 3], "{out:?}");
  }

  #[test]
  fn footnote_indices_continue_across_source_groups() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["one \\footnote{a}\n", "two \\footnote{b}\n"]);

    // Act
    let (layout, _headings) = lower_body(&style, &analyzed);

    // Assert
    let indices: Vec<u32> = layout
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Footnote { index, .. } => return Some(*index),
        _ => return None,
      })
      .collect();
    assert_eq!(indices, vec![0, 1], "脚注の出現 index はグループを跨いで通し番号: {layout:?}");
  }

  #[test]
  fn labeled_display_math_emits_label_anchor() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "\\begin{equation}[label=eq:foo]\na\n\\end{equation}\n");

    // Assert
    assert!(
      matches!(out.first(), Some(LayoutNode::Anchor(AnchorMark::Label(l))) if l.as_str() == "eq:foo"),
      "先頭は Label アンカー: {out:?}"
    );
  }

  #[test]
  fn unlabeled_display_math_emits_no_anchor() {
    // Arrange
    let style = ReadStyle::default();

    // Act
    let out = lower_source(&style, "\\begin{equation}\na\n\\end{equation}\n");

    // Assert
    assert!(!out.iter().any(|n| matches!(n, LayoutNode::Anchor(_))), "アンカーは出ない: {out:?}");
  }

  #[test]
  fn default_font_size_reflects_core_font_size() {
    // Arrange
    let mut style = ReadStyle::default();
    style.text.font_size = Length::pt(18.0);

    // Act
    let out = lower_source(&style, "x\n");

    // Assert
    let LayoutNode::Text(_, text_style) = &out[0] else {
      panic!("先頭は Text であるべき: {out:?}");
    };
    assert_eq!(text_style.font_size, Length::pt(18.0));
  }

  #[test]
  fn numbering_continues_across_sources() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["\\chapter{A}\n", "\\chapter{B}\n"]);

    // Act
    let (_layout, headings) = lower_body(&style, &analyzed);

    // Assert
    assert_eq!(headings.len(), 2, "{headings:?}");
    assert_eq!(headings[0].number, "1", "1 グループ目の chapter は 1");
    assert_eq!(headings[1].number, "2", "2 グループ目の chapter は連番の 2: {headings:?}");
  }

  #[test]
  fn ref_resolves_across_sources() {
    // Arrange
    fn contains_internal_link(nodes: &[LayoutNode], target: &str) -> bool {
      return nodes.iter().any(|n| match n {
        LayoutNode::Link {
          target: LinkTarget::Internal(t),
          ..
        } => return *t == AnchorId::Label(LabelId::new(target)),
        LayoutNode::VBox { children, .. } => {
          return contains_internal_link(children, target);
        },
        _ => return false,
      });
    }

    let style = ReadStyle::default();
    let analyzed = analyzed_sources(&["\\chapter[label=ch:intro]{Intro}\n", "\\ref{ch:intro}\n"]);

    // Act
    let (layout, _headings) = lower_body(&style, &analyzed);

    // Assert
    assert!(contains_internal_link(&layout, "ch:intro"), "跨りの \\ref が解決されるはず: {layout:?}");
  }

  #[test]
  fn heading_number_uses_style_number_format() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed("\\chapter{C}\n\n\\section{S}\n\n\\section{S2}\n");

    // Act
    let (_layout, headings) = lower_body(&style, &analyzed);

    // Assert
    let numbers: Vec<&str> = headings.iter().map(|h| return h.number.as_str()).collect();
    assert_eq!(numbers, vec!["1", "1.1", "1.2"], "section は既定で \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn heading_title_plain_resolves_embedded_ref() {
    // Arrange
    let style = ReadStyle::default();
    let analyzed = analyzed("\\chapter[label=ch:intro]{Intro}\n\n\\section{見出し \\ref{ch:intro}}\n");

    // Act
    let (_layout, headings) = lower_body(&style, &analyzed);

    // Assert
    assert_eq!(headings[1].title_plain, "見出し Chapter 1", "タイトル中の \\ref も表示文字列になる");
  }

  #[test]
  fn heading_title_plain_uses_generated_citation_display() {
    // Arrange — 見出しタイトルの `\cite` は、しおり・目次では CSL 整形済みの表示を辿る
    let style = ReadStyle::default();
    let analyzed = analyzed("\\section{結論 \\cite{kwan2014}}\n");
    let site = analyzed.citation_sites().next().expect("引用箇所が 1 件あるはず");
    let document =
      analyzed.with_citations_for_test(vec![(site, vec![GeneratedInline::Text("[1]".to_string())])], Vec::new());
    let ctx = LoweringContext::new(&style);

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    assert_eq!(headings[0].title_plain, "結論 [1]", "{headings:?}");
  }
}
