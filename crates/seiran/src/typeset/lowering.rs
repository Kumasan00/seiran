//! Lowering 層: 解決済みドキュメント（`resolve::ResolvedDocument`）→ `LayoutNode` 変換
//!
//! ラベル・カウンタの解決（採番・`\ref` の存在検証）は `resolve` クレートが済ませているため、
//! この層は「解決済みの構造値を style の表示側フィールドで文字列にして箱に積む」だけを行う。
//! 意味解析を行わないので、この層に失敗はない（`Result` を返さない）。

use config::Style as ReadStyle;
use model::{LabelId, Length};
use resolve::{ResolvedDocument, ResolvedInline, ResolvedNode};
use tracing::debug;

mod counter;
mod figure;
mod float;
mod heading;
mod inline;
mod layout_node;
mod list;
mod math;
mod paragraph;
mod placeholder;
mod quote;
mod table;
mod template;
mod theorem;
mod title_page;

pub use counter::per_page_footnote_numbers;
pub use layout_node::{LayoutNode, MathBlockRow, TableCellLayout, TableLayout, TableRowLayout, TextStyle};
pub(crate) use title_page::{TitlePageMetadata, lower_title_page};

/// Lowering のコンテキスト
pub struct LoweringContext<'a> {
  /// スタイル設定への参照（`config/style.toml` 由来 + figment デフォルト）
  pub style: &'a ReadStyle,
  /// 本文段落の既定フォント種別
  pub body_font_kind: model::FontKind,
  /// 段落先頭行の字下げ量
  pub first_line_indent: model::Length,
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
  pub fn new(style: &'a ReadStyle) -> Self {
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
  pub fn with_image_defaults(mut self, image_max_dpi: u32, image_downsample: bool) -> Self {
    self.image_max_dpi = image_max_dpi;
    self.image_downsample = image_downsample;
    return self;
  }

  /// 脚注の表示番号の上書きマップを与えた文脈を返す
  #[must_use]
  pub fn with_footnote_numbers(mut self, numbers: &'a [u32]) -> Self {
    self.footnote_numbers = Some(numbers);
    return self;
  }

  /// 本文段落の既定フォント種別だけを差し替えた派生文脈を返す
  #[must_use]
  pub fn with_body_font_kind(&self, body_font_kind: model::FontKind) -> LoweringContext<'a> {
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
  pub fn with_first_line_indent(&self, first_line_indent: model::Length) -> LoweringContext<'a> {
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
  pub fn with_list_depth(&self, list_depth: usize) -> LoweringContext<'a> {
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
  pub fn default_font_size(&self) -> Length { return self.style.text.font_size; }
}

/// 見出し 1 件の記録（PDF しおり・目次生成が消費する）
#[derive(Debug, Clone, PartialEq)]
pub struct HeadingRecord {
  /// 見出しの文書順インデックス（0 始まり）
  pub index: usize,
  /// 見出しレベル
  pub level: model::HeadingLevel,
  /// 書式化済みの見出し番号（無採番の見出しは空文字列）
  pub number: String,
  /// 見出しタイトルのプレーンテキスト（`\ref` 解決済み）
  pub title_plain: String,
}

/// 子 module のテストが [`LoweringState`] を組み立てるための最小ヘルパ
#[cfg(test)]
pub(super) mod test_support {
  use std::collections::HashMap;

  use model::LabelId;
  use resolve::{CounterValue, ResolvedDocument};

  /// ラベル → カウンタ値の対応だけを持つ最小の解決済みドキュメントを作る
  ///
  /// `\ref` / `{of}` の表示文字列化しか使わないテスト（ほとんどの子 module のテスト）は
  /// これで足りる。`groups` / `headings` を実際に走査するテストは `resolve::resolve_project`
  /// を通した本物の `ResolvedDocument` を使う。
  pub(crate) fn document(counter_values: &[(&str, CounterValue)]) -> ResolvedDocument {
    return ResolvedDocument {
      groups: Vec::new(),
      bibliography: Vec::new(),
      headings: Vec::new(),
      counter_values: counter_values
        .iter()
        .map(|(label, value)| return (LabelId::new(*label), value.clone()))
        .collect::<HashMap<_, _>>(),
    };
  }
}

/// 走査中に更新される可変状態（旧 `CounterRegistry` の可変部分の置き換え）
///
/// 採番そのものは `resolve` が済ませているため、ここに残るのは「解決済みドキュメントへの
/// 参照」と「文書順に払い出す 2 種類の通し index」だけになる。
pub(super) struct LoweringState<'a> {
  /// 解決済みドキュメント（`\ref` の表示文字列化と見出しカウンタ値の参照に使う）
  document: &'a ResolvedDocument,
  /// これまでに払い出した脚注の個数（次の脚注の出現 index になる）
  footnote_count: u32,
  /// これまでに払い出した見出しの個数（次の見出しの文書順インデックスになる）
  heading_count: usize,
}

impl<'a> LoweringState<'a> {
  /// 解決済みドキュメントに対する初期状態を作る
  pub(super) fn new(document: &'a ResolvedDocument) -> Self {
    return LoweringState {
      document,
      footnote_count: 0,
      heading_count: 0,
    };
  }

  /// 脚注を 1 つ数え、その出現 index（0 起点）を返す
  pub(super) fn next_footnote_index(&mut self) -> u32 {
    let index = self.footnote_count;
    self.footnote_count += 1;
    return index;
  }

  /// 見出しを 1 つ数え、その文書順インデックス（0 起点）を返す
  ///
  /// `resolve::resolve_group` と走査順が一致しているため、この値は
  /// [`ResolvedDocument::headings`] の添字（= `ResolvedHeading::key`）と一致する。
  fn next_heading_index(&mut self) -> usize {
    let index = self.heading_count;
    self.heading_count += 1;
    return index;
  }

  /// `\ref` / `proof` の `[of=...]` の参照先表示文字列を作る
  ///
  /// # Panics
  ///
  /// 参照先が `ResolvedDocument::counter_values` に無い場合にパニックします（`resolve` の
  /// 存在検証を通過した `LabelId` しか到達しないため、通常は起こりません）。
  pub(super) fn ref_display(&self, style: &ReadStyle, target: &LabelId) -> String {
    let Some(value) = self.document.counter_values.get(target) else {
      unreachable!("参照先の存在は resolve::validate_refs が保証している: {target:?}")
    };
    return counter::format_ref_display(style, value);
  }
}

/// 解決済みドキュメントをレイアウトノードに変換し、見出し記録（PDF しおり・目次生成用）も返す
#[must_use]
pub fn lower_sources_with_headings(
  ctx: &LoweringContext,
  document: &ResolvedDocument,
) -> (Vec<LayoutNode>, Vec<HeadingRecord>) {
  let mut state = LoweringState::new(document);
  let mut result = Vec::new();
  // グループの起源（`ResolvedGroup::source_id`）はエラー帰属のための情報で、`resolve` が
  // 診断を出し終えた後の lowering では読む先が無い（診断を出さないので文脈に持たない）。
  for group in &document.groups {
    result.extend(lower_nodes_inner(ctx, &group.nodes, &mut state));
  }
  // 書誌は常に groups の後に lower する（`next_heading_index()` が `document.headings` の
  // 添字と一致する前提は、resolve が書誌を最後に解決する順序と揃っていることに依存する）
  result.extend(lower_nodes_inner(ctx, &document.bibliography, &mut state));

  let headings = document
    .headings
    .iter()
    .map(|heading| {
      return HeadingRecord {
        index: heading.key.index(),
        level: heading.level,
        number: heading
          .counter_value
          .as_ref()
          .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value)),
        title_plain: resolved_inlines_to_plain_text(&heading.title, ctx.style, &state),
      };
    })
    .collect();

  let input_node_count: usize =
    document.groups.iter().map(|group| return group.nodes.len()).sum::<usize>() + document.bibliography.len();
  debug!(input_node_count, layout_node_count = result.len(), "lowering が完了しました");
  return (result, headings);
}

/// `nodes` を順に [`lower_node_indexed`] へ渡す内部ウォーク（本体）
pub(super) fn lower_nodes_inner(
  ctx: &LoweringContext,
  nodes: &[ResolvedNode],
  state: &mut LoweringState,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node_indexed(ctx, node, state));
  }
  return result;
}

/// 単一の `ResolvedNode` をレイアウトノードに変換する
fn lower_node_indexed(ctx: &LoweringContext, node: &ResolvedNode, state: &mut LoweringState) -> Vec<LayoutNode> {
  match node {
    ResolvedNode::Heading {
      level,
      title,
      label,
      ..
    } => {
      // 見出しの文書順インデックス。`resolve` 側の走査順と一致するので、再帰
      // （quote / theorem / list item 本体）を挟んでも `document.headings` の添字と揃う
      // （`heading_anchor_key` の暗黙 destination キー採番に使う）。
      let heading_index = state.next_heading_index();
      let number = heading_number(ctx, state.document, heading_index);
      return heading::lower_heading(ctx, *level, &number, title, label.clone(), heading_index, state);
    },
    ResolvedNode::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines, state);
    },
    ResolvedNode::List {
      ordered,
      items,
      start,
      item_gap,
    } => {
      return list::lower_list(ctx, *ordered, items, *start, *item_gap, state);
    },
    ResolvedNode::Theorem {
      class,
      title,
      body,
      of,
      label,
      counter_value,
      ..
    } => {
      let number = counter_value.as_ref().map(|value| return counter::format_counter_value(ctx.style, value));
      return theorem::lower_theorem(
        ctx,
        *class,
        number.as_deref(),
        title.as_deref(),
        body,
        of.as_ref().map(|target| return &target.target),
        label.as_ref(),
        state,
      );
    },
    ResolvedNode::Quote { kind, body } => {
      return quote::lower_quote(ctx, *kind, body, state);
    },
    ResolvedNode::Rule { width, height } => {
      return vec![LayoutNode::Rule {
        width: *width,
        height: *height,
      }];
    },
    ResolvedNode::PageBreak => {
      return vec![LayoutNode::PageBreak];
    },
    ResolvedNode::Space(length) => {
      return vec![LayoutNode::Kern { length: *length }];
    },
    ResolvedNode::Anchor(target) => {
      return vec![LayoutNode::Anchor(model::AnchorMark::Citation(
        target.clone(),
      ))];
    },
    ResolvedNode::MathBlock {
      kind,
      rows,
      label,
      counter_value,
      ..
    } => {
      let block = &ctx.style.math.block;
      let math_block = math::lower_math_block(ctx, *kind, rows, counter_value.as_ref());
      let mut nodes = vec![
        LayoutNode::Vkern {
          length: block.top_margin,
        },
        math_block,
        LayoutNode::Vkern {
          length: block.bottom_margin,
        },
      ];
      // ラベル付き行（`equation` の `[label=...]`、`align` / `gather` の行末 `\label{...}`）の `\ref`
      // 到達先アンカーを先頭に付ける。複数行がラベルを持つ場合も、いずれもブロック先頭座標に解決される。
      // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する。
      let mut anchor_labels: Vec<&LabelId> = Vec::new();
      if let Some(env_label) = label.as_ref() {
        anchor_labels.push(env_label);
      }
      // 行ラベルは逆順で積む（「後から prepend」を繰り返す旧実装と同じ最終順序を 1 パスで
      // 再現するため。`with_label_anchors` の doc comment も参照）
      anchor_labels.extend(rows.iter().rev().filter_map(|row| return row.label.as_ref()));
      nodes = with_label_anchors(&anchor_labels, nodes);
      return nodes;
    },
    ResolvedNode::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      label,
      counter_value,
      ..
    } => {
      let caption_arg = caption.as_deref().map(|inlines| return (*caption_position, inlines));
      let overrides = figure::ImageOverrides {
        dpi: *dpi,
        downsample: *downsample,
      };
      let number = counter::format_counter_value(ctx.style, counter_value);
      let nodes = figure::lower_figure(ctx, image_path, *width, *height, overrides, caption_arg, &number, state);
      return with_label_anchor(label.as_ref(), nodes);
    },
    ResolvedNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      breakable,
      label,
      counter_value,
      ..
    } => {
      let caption_arg = caption.as_deref().map(|inlines| return (*caption_position, inlines));
      let number = counter::format_counter_value(ctx.style, counter_value);
      let nodes = table::lower_table(ctx, columns, widths, head, rows, caption_arg, &number, *breakable, state);
      return with_label_anchor(label.as_ref(), nodes);
    },
  }
}

/// `document.headings[index]` の見出し番号を表示文字列にする（無採番の見出しは空文字列）
fn heading_number(ctx: &LoweringContext, document: &ResolvedDocument, index: usize) -> String {
  let Some(heading) = document.headings.get(index) else {
    unreachable!("見出しの走査順は resolve::resolve_group と一致するので添字は必ず存在する: {index}")
  };
  return heading
    .counter_value
    .as_ref()
    .map_or_else(String::new, |value| return counter::format_counter_value(ctx.style, value));
}

/// ラベル付きブロック（図・表・ディスプレイ数式）の先頭に `\ref` 到達先アンカーを付与する
fn with_label_anchor(label: Option<&LabelId>, nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let Some(label) = label else {
    return nodes;
  };
  let mut result = Vec::with_capacity(nodes.len() + 1);
  result.push(LayoutNode::Anchor(model::AnchorMark::Label(label.clone())));
  result.extend(nodes);
  return result;
}

/// 複数のラベルを先頭からこの順でアンカーとして 1 回の構築でまとめて付与する
fn with_label_anchors(labels: &[&LabelId], nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  if labels.is_empty() {
    return nodes;
  }
  let mut result = Vec::with_capacity(nodes.len() + labels.len());
  result.extend(labels.iter().map(|label| return LayoutNode::Anchor(model::AnchorMark::Label((*label).clone()))));
  result.extend(nodes);
  return result;
}

/// 解決済みインライン列をプレーンテキストへ畳む（見出しタイトルのしおり・目次表示用）
///
/// `model::InlineNode::try_to_plain_text` の解決済み版。バリアントごとの扱い（数式は
/// `"[Math]"`、脚注・索引は空、`\cite` は整形済みラベルを辿る等）は同じに保つ。
fn resolved_inlines_to_plain_text(inlines: &[ResolvedInline], style: &ReadStyle, state: &LoweringState) -> String {
  let mut out = String::new();
  for inline in inlines {
    match inline {
      ResolvedInline::Text(s) => out.push_str(s),
      ResolvedInline::Styled { children, .. }
      | ResolvedInline::Colored { children, .. }
      | ResolvedInline::Link { children, .. }
      | ResolvedInline::InternalLink { children, .. }
      | ResolvedInline::Cite {
        label: children, ..
      } => {
        out.push_str(&resolved_inlines_to_plain_text(children, style, state));
      },
      ResolvedInline::InlineMath(_) => out.push_str("[Math]"),
      ResolvedInline::Symbol(ch) => out.push(*ch),
      ResolvedInline::LineBreak => out.push('\n'),
      // 脚注本体・索引マーカーは見出しのプレーンテキスト抽出には含めない（NoIndent と同じ空扱い）
      ResolvedInline::NoIndent | ResolvedInline::Footnote { .. } | ResolvedInline::Index { .. } => {},
      ResolvedInline::Ref { target, .. } => out.push_str(&state.ref_display(style, target)),
    }
  }
  return out;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use model::{
    DocNode, HeadingLevel, InlineNode, Length, ListItem, MathEnvKind, MathNode, MathRow, QuoteKind, SourceId,
  };
  use resolve::{SemanticDocument, SemanticGroup};

  use super::*;

  /// `DocNode` 列を `resolve::resolve_project` に通して解決済みドキュメントにするテストヘルパ
  ///
  /// lowering の入力は解決済みツリーなので、`DocNode` を組み立てるテストはこの 2 段構成になる。
  fn resolved(style: &ReadStyle, groups: &[&[DocNode]]) -> ResolvedDocument {
    let semantic = SemanticDocument {
      groups: groups
        .iter()
        .enumerate()
        .map(|(index, nodes)| {
          return SemanticGroup {
            nodes,
            source_id: SourceId::new(index),
          };
        })
        .collect(),
      bibliography: &[],
    };
    return resolve::resolve_project(&semantic, style).expect("解決できる入力のはず");
  }

  /// 1 グループの `DocNode` 列を lower して `LayoutNode` 列だけを返すテストヘルパ
  fn lower_group(ctx: &LoweringContext, style: &ReadStyle, nodes: &[DocNode]) -> Vec<LayoutNode> {
    let document = resolved(style, &[nodes]);
    let (layout, _headings) = lower_sources_with_headings(ctx, &document);
    return layout;
  }

  /// 1 行 1 セルの `equation` 相当 `DocNode::MathBlock` を作るテストヘルパ
  fn equation_block(numbered: bool, label: Option<&str>) -> DocNode {
    return DocNode::MathBlock {
      kind: MathEnvKind::Equation,
      rows: vec![MathRow {
        cells: vec![vec![MathNode::Text("a".to_string())]],
        numbered,
        label: label.map(str::to_string),
        label_span: None,
      }],
      numbered: false,
      label: None,
      span: model::Span::DUMMY,
    };
  }

  /// レイアウトノード木を再帰的に走査し、`LineBreak` が含まれるか調べるヘルパ
  fn contains_line_break(nodes: &[LayoutNode]) -> bool {
    return nodes.iter().any(|n| match n {
      LayoutNode::LineBreak => return true,
      LayoutNode::VBox { children, .. } | LayoutNode::HBox { children, .. } | LayoutNode::Raise { children, .. } => {
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
  fn test_lower_space() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Space(Length::pt(5.0))];

    // Act
    let result = lower_group(&ctx, &style, &nodes);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Kern { length } => assert!((length.to_pt() - 5.0).abs() < f32::EPSILON),
      other => panic!("Expected Kern, got {other:?}"),
    }
  }

  #[test]
  fn test_lower_anchor() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Anchor(model::CitationId::new("foo"))];

    // Act
    let result = lower_group(&ctx, &style, &nodes);

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], LayoutNode::Anchor(model::AnchorMark::Citation(k)) if k.as_str() == "foo"));
  }

  #[test]
  fn test_lower_page_break() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::PageBreak];

    // Act
    let result = lower_group(&ctx, &style, &nodes);

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn test_lower_rule() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Rule {
      width: Length::pt(100.0),
      height: Length::pt(1.0),
    }];

    // Act
    let result = lower_group(&ctx, &style, &nodes);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Rule { width, height } => {
        assert!((width.to_pt() - 100.0).abs() < f32::EPSILON);
        assert!((height.to_pt() - 1.0).abs() < f32::EPSILON);
      },
      other => panic!("Expected Rule, got {other:?}"),
    }
  }

  #[test]
  fn lower_inline_math_replaces_placeholder() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Paragraph(vec![InlineNode::InlineMath(vec![
      MathNode::Text("x".to_string()),
      MathNode::Superscript(Box::new(MathNode::Text("2".to_string()))),
    ])])];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

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
    let ctx = LoweringContext::new(&style);
    let nodes = [equation_block(false, None)];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

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
    style.counters.equation.number_format = "{n}".to_string();
    let ctx = LoweringContext::new(&style);
    let nodes = [equation_block(true, None)];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

    // Assert
    let Some(LayoutNode::MathBlock { rows, .. }) = out.get(1) else {
      panic!("中央に MathBlock があるべき: {out:?}");
    };
    assert_eq!(rows.len(), 1, "equation は 1 行: {rows:?}");
    let number = rows[0].number.as_ref().expect("採番された行は番号ボックスを持つ");
    assert!(
      matches!(&number[0], LayoutNode::Text(t, _) if t == "(1)"),
      "番号ボックスは Text(\"(1)\"): {number:?}"
    );
  }

  #[test]
  fn lower_nodes_dispatches_each_variant_in_order() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("H".to_string())]),
      DocNode::Paragraph(vec![InlineNode::Text("P".to_string())]),
      DocNode::List {
        ordered: false,
        items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
          InlineNode::Text("L".to_string()),
        ])])],
        start: None,
        item_gap: None,
      },
      DocNode::PageBreak,
    ];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

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
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Top1".to_string())]),
      DocNode::Quote {
        kind: QuoteKind::Quote,
        body: vec![DocNode::heading(
          HeadingLevel::Section,
          vec![InlineNode::Text("Nested".to_string())],
        )],
      },
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Top2".to_string())]),
    ];
    let document = resolved(&style, &[&nodes]);

    // Act
    let (layout, headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    assert_eq!(headings.len(), 3, "見出しは 3 件記録されるはず: {headings:?}");
    let indices: Vec<usize> = headings.iter().map(|h| return h.index).collect();
    assert_eq!(indices, vec![0, 1, 2], "見出し index は文書順に連番のはず: {headings:?}");
    // `AnchorMark::Heading` の key が `HeadingRecord::index` と 1:1 かつ同順で対応することを確かめる
    // （`resolve` 側の走査順とこちらの走査順が食い違うと目次の内部リンクが静かに壊れる。集合一致では
    // key が入れ替わっていても検出できないため、ソートせず順序も含めて比較する）。
    let anchor_keys = collect_heading_anchor_keys(&layout);
    assert_eq!(anchor_keys, indices, "アンカーの key は見出し記録の index と順序込みで一致するはず: {layout:?}");
  }

  /// レイアウトノード木から `AnchorMark::Heading` の key（文書順インデックス）を集める
  fn collect_heading_anchor_keys(nodes: &[LayoutNode]) -> Vec<usize> {
    let mut keys = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Anchor(model::AnchorMark::Heading { key, .. }) => keys.push(key.index()),
        LayoutNode::VBox { children, .. } | LayoutNode::HBox { children, .. } => {
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
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Heading".to_string())]),
      DocNode::Paragraph(vec![InlineNode::Text("Para".to_string())]),
      DocNode::List {
        ordered: true,
        items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
          InlineNode::Text("Item".to_string()),
        ])])],
        start: None,
        item_gap: None,
      },
    ];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

    // Assert
    assert!(!contains_line_break(&out), "段落内 \\\\ 以外で LineBreak は出力されない: {out:?}");
  }

  #[test]
  fn footnotes_across_paragraphs_number_sequentially() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let footnote = |text: &str| {
      return InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      };
    };
    let nodes = vec![
      DocNode::Paragraph(vec![InlineNode::Text("one ".to_string()), footnote("a")]),
      DocNode::Paragraph(vec![InlineNode::Text("two ".to_string()), footnote("b")]),
      DocNode::Paragraph(vec![InlineNode::Text("three ".to_string()), footnote("c")]),
    ];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

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
    let ctx = LoweringContext::new(&style);
    let footnote = |text: &str| {
      return DocNode::Paragraph(vec![InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      }]);
    };
    let g0 = [footnote("a")];
    let g1 = [footnote("b")];
    let document = resolved(&style, &[&g0, &g1]);

    // Act
    let (layout, _headings) = lower_sources_with_headings(&ctx, &document);

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
    let ctx = LoweringContext::new(&style);
    let nodes = [equation_block(true, Some("eq:foo"))];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

    // Assert
    assert!(
      matches!(out.first(), Some(LayoutNode::Anchor(model::AnchorMark::Label(l))) if l.as_str() == "eq:foo"),
      "先頭は Label アンカー: {out:?}"
    );
  }

  #[test]
  fn unlabeled_display_math_emits_no_anchor() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [equation_block(true, None)];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

    // Assert
    assert!(!out.iter().any(|n| matches!(n, LayoutNode::Anchor(_))), "アンカーは出ない: {out:?}");
  }

  #[test]
  fn default_font_size_reflects_core_font_size() {
    // Arrange
    let mut style = config::Style::default();
    style.text.font_size = Length::pt(18.0);
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Paragraph(vec![InlineNode::Text("x".to_string())])];

    // Act
    let out = lower_group(&ctx, &style, &nodes);

    // Assert
    assert_eq!(ctx.default_font_size(), Length::pt(18.0));
    let LayoutNode::Text(_, text_style) = &out[0] else {
      panic!("先頭は Text であるべき: {out:?}");
    };
    assert_eq!(text_style.font_size, Length::pt(18.0));
  }

  /// ラベル付き Chapter 見出しを作るテストヘルパ
  fn labeled_chapter(title: &str, label: &str) -> DocNode {
    return DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::Text(title.to_string())],
      label: Some(label.to_string()),
      span: model::Span::DUMMY,
    };
  }

  #[test]
  fn numbering_continues_across_sources() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = [DocNode::heading(
      HeadingLevel::Chapter,
      vec![InlineNode::Text("A".to_string())],
    )];
    let g1 = [DocNode::heading(
      HeadingLevel::Chapter,
      vec![InlineNode::Text("B".to_string())],
    )];
    let document = resolved(&style, &[&g0, &g1]);

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

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
          target: model::LinkTarget::Internal(t),
          ..
        } => return *t == model::AnchorId::Label(LabelId::new(target)),
        LayoutNode::VBox { children, .. } | LayoutNode::HBox { children, .. } => {
          return contains_internal_link(children, target);
        },
        _ => return false,
      });
    }

    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = [labeled_chapter("Intro", "ch:intro")];
    let g1 = [DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      span: model::Span::DUMMY,
    }])];
    let document = resolved(&style, &[&g0, &g1]);

    // Act
    let (layout, _headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    assert!(contains_internal_link(&layout, "ch:intro"), "跨りの \\ref が解決されるはず: {layout:?}");
  }

  #[test]
  fn heading_number_uses_style_number_format() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Chapter, vec![InlineNode::Text("C".to_string())]),
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("S".to_string())]),
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("S2".to_string())]),
    ];
    let document = resolved(&style, &[&nodes]);

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    let numbers: Vec<&str> = headings.iter().map(|h| return h.number.as_str()).collect();
    assert_eq!(numbers, vec!["1", "1.1", "1.2"], "section は既定で \"{{chapter}}.{{n}}\"");
  }

  #[test]
  fn unnumbered_heading_has_empty_number() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = [DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: false,
      title: vec![InlineNode::Text("Preface".to_string())],
      label: None,
      span: model::Span::DUMMY,
    }];
    let document = resolved(&style, &[&nodes]);

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    assert_eq!(headings[0].number, "", "無採番の見出しの番号は空文字列");
  }

  #[test]
  fn heading_title_plain_resolves_embedded_ref() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      labeled_chapter("Intro", "ch:intro"),
      DocNode::heading(
        HeadingLevel::Section,
        vec![
          InlineNode::Text("見出し ".to_string()),
          InlineNode::Ref {
            label: "ch:intro".to_string(),
            span: model::Span::DUMMY,
          },
        ],
      ),
    ];
    let document = resolved(&style, &[&nodes]);

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &document);

    // Assert
    assert_eq!(headings[1].title_plain, "見出し Chapter 1", "タイトル中の \\ref も表示文字列になる");
  }
}
