//! `AnalyzedDocument` と引用の生成物から、lowering の入力 `ResolvedDocument` を組み立てる。
//!
//! **これは #325 で消える一時的な足場**。lowering が `AnalyzedDocument` を直接読むようになれば
//! `Resolved*` の木ごと不要になるので、ここを一般化・抽象化しないこと。
//!
//! この module は意味を一切決めない。採番・ラベル登録・`\ref` の解決はすべて
//! [`crate::resolve::analyze`] が済ませており、ここは HIR を走査しながら fact を引いて
//! 木へ写すだけ（失敗しないので `Result` を返さない）。
//!
//! 書誌（`citation::generate_citations` の生成物）だけは HIR ではなく `DocNode` で来るため、
//! 専用の小さな変換を持つ。書誌は無採番の見出しと段落・アンカーしか含まず、ラベルも `\ref` も
//! 持たないので、必要なのは見出しキーを 1 つ振ることだけ。

use std::collections::HashMap;

use crate::{
  model::{
    DocNode, HeadingKey, HirGroup, HirInline, HirInlineKind, HirListItem, HirMathRow, HirNode, HirNodeKind,
    HirTableRow, InlineNode, LabelId, NodeId, NodeMap, SourceMap, to_math_nodes,
  },
  resolve::{
    counter::CounterValue,
    document::{ResolvedDocument, ResolvedGenerated, ResolvedGroup, ResolvedHeading},
    facts::AnalyzedDocument,
    inline::{IndexKey, ResolvedInline},
    node::{ResolvedListItem, ResolvedMathRow, ResolvedNode, ResolvedProofTarget, ResolvedTableCell, ResolvedTableRow},
  },
};

/// 解析済みドキュメントと引用の生成物から `ResolvedDocument` を組み立てる
///
/// 意味の確定は `analyze` で終わっているため、この関数は失敗しない。
#[must_use]
pub fn build_resolved_document(
  analyzed: &AnalyzedDocument,
  citation_displays: &NodeMap<Vec<InlineNode>>,
  bibliography: &[DocNode],
) -> ResolvedDocument {
  let mut builder = Builder {
    analyzed,
    locations: analyzed.hir().locations(),
    heading_titles: NodeMap::default(),
  };

  let groups: Vec<ResolvedGroup> = analyzed
    .hir()
    .groups()
    .iter()
    .map(|group: &HirGroup| {
      return ResolvedGroup {
        nodes: builder.nodes(&group.nodes),
        source_id: group.source_id,
      };
    })
    .collect();

  // 見出し一覧は facts の順（= analyze が振った `HeadingKey` の順）で組む。走査順に依存しない。
  let mut headings: Vec<ResolvedHeading> = analyzed
    .headings()
    .iter()
    .map(|facts| {
      let Some(title) = builder.heading_titles.get(facts.node) else {
        unreachable!("見出しのタイトルは groups の走査で必ず収集される: {:?}", facts.node)
      };
      return ResolvedHeading {
        key: facts.key,
        level: facts.level,
        counter_value: facts.counter_value.clone(),
        title: title.clone(),
      };
    })
    .collect();

  let bibliography_nodes = bibliography_to_resolved(bibliography, headings.len(), &mut headings);

  let mut displays: NodeMap<Vec<ResolvedInline>> = NodeMap::default();
  for (site, display) in citation_displays.iter() {
    displays.insert(site, generated_inlines(display));
  }

  return ResolvedDocument {
    groups,
    generated: ResolvedGenerated {
      citation_displays: displays,
      bibliography: bibliography_nodes,
    },
    headings,
    counter_values: label_counter_values(analyzed),
  };
}

/// ラベル → カウンタ構造値の対応表を facts から組み立てる
fn label_counter_values(analyzed: &AnalyzedDocument) -> HashMap<LabelId, CounterValue> {
  let mut values = HashMap::new();
  for (node, label) in analyzed.declared_labels() {
    let Some(value) = analyzed.counter_value(node) else {
      unreachable!("ラベル宣言ノードは必ず採番済み: {node:?}")
    };
    values.insert(label.clone(), value.clone());
  }
  return values;
}

/// HIR を走査して `ResolvedNode` を組み立てる（fact を引くだけで、意味は決めない）
struct Builder<'a> {
  /// fact の参照先
  analyzed: &'a AnalyzedDocument,
  /// `NodeId` → ソース位置の対応表
  locations: &'a SourceMap,
  /// 見出しノード → 解決済みタイトル（`ResolvedDocument::headings` の組み立てに使う）
  heading_titles: NodeMap<Vec<ResolvedInline>>,
}

impl Builder<'_> {
  /// ブロックノード列を変換する
  fn nodes(&mut self, nodes: &[HirNode]) -> Vec<ResolvedNode> {
    return nodes.iter().map(|node| return self.node(node)).collect();
  }

  /// 単一のブロックノードを変換する
  fn node(&mut self, node: &HirNode) -> ResolvedNode {
    let span = self.locations.location(node.id).span;
    match &node.kind {
      HirNodeKind::Heading {
        level,
        title,
        label,
      } => {
        let resolved_title = self.inlines(title);
        self.heading_titles.insert(node.id, resolved_title.clone());
        return ResolvedNode::Heading {
          level: *level,
          // HIR の見出しは常に採番対象（無採番の見出しは書誌の合成見出しだけ）。
          numbered: true,
          title: resolved_title,
          label: self.declared_label(node.id, label.as_ref()),
          key: self.heading_key(node.id),
          span,
        };
      },
      HirNodeKind::Paragraph(inlines) => return ResolvedNode::Paragraph(self.inlines(inlines)),
      HirNodeKind::List {
        ordered,
        items,
        start,
        item_gap,
      } => {
        return ResolvedNode::List {
          ordered: *ordered,
          items: items.iter().map(|item| return self.list_item(item)).collect(),
          start: *start,
          item_gap: *item_gap,
        };
      },
      HirNodeKind::MathBlock {
        kind,
        rows,
        numbered,
        label,
      } => {
        return ResolvedNode::MathBlock {
          kind: *kind,
          rows: rows.iter().map(|row| return self.math_row(row)).collect(),
          numbered: *numbered,
          label: self.declared_label(node.id, label.as_ref()),
          counter_value: self.analyzed.counter_value(node.id).cloned(),
          span,
        };
      },
      HirNodeKind::Figure {
        image_path,
        width,
        height,
        dpi,
        downsample,
        caption,
        caption_position,
        label,
      } => {
        let Some(counter_value) = self.analyzed.counter_value(node.id).cloned() else {
          unreachable!("図は必ず採番される: {:?}", node.id)
        };
        return ResolvedNode::Figure {
          image_path: image_path.clone(),
          width: *width,
          height: *height,
          dpi: *dpi,
          downsample: *downsample,
          caption: caption.as_ref().map(|inlines| return self.inlines(inlines)),
          caption_position: *caption_position,
          label: self.declared_label(node.id, label.as_ref()),
          counter_value,
          span,
        };
      },
      HirNodeKind::Table {
        columns,
        widths,
        head,
        rows,
        caption,
        caption_position,
        label,
        breakable,
      } => {
        let Some(counter_value) = self.analyzed.counter_value(node.id).cloned() else {
          unreachable!("表は必ず採番される: {:?}", node.id)
        };
        return ResolvedNode::Table {
          columns: columns.clone(),
          widths: widths.clone(),
          head: head.iter().map(|row| return self.table_row(row)).collect(),
          rows: rows.iter().map(|row| return self.table_row(row)).collect(),
          caption: caption.as_ref().map(|inlines| return self.inlines(inlines)),
          caption_position: *caption_position,
          label: self.declared_label(node.id, label.as_ref()),
          counter_value,
          span,
          breakable: *breakable,
        };
      },
      HirNodeKind::Theorem {
        class,
        title,
        body,
        of,
        label,
      } => {
        return ResolvedNode::Theorem {
          class: *class,
          title: title.clone(),
          body: self.nodes(body),
          of: of.as_ref().map(|target| {
            return ResolvedProofTarget {
              target: self.analyzed.reference_target(target.id).clone(),
              span: self.locations.location(target.id).span,
            };
          }),
          label: self.declared_label(node.id, label.as_ref()),
          counter_value: self.analyzed.counter_value(node.id).cloned(),
          span,
        };
      },
      HirNodeKind::Quote { kind, body } => {
        return ResolvedNode::Quote {
          kind: *kind,
          body: self.nodes(body),
        };
      },
      HirNodeKind::Rule { width, height } => {
        return ResolvedNode::Rule {
          width: *width,
          height: *height,
        };
      },
      HirNodeKind::PageBreak => return ResolvedNode::PageBreak,
      HirNodeKind::Space(length) => return ResolvedNode::Space(*length),
    }
  }

  /// 見出しの `HeadingKey` を facts から引く
  fn heading_key(&self, node: NodeId) -> HeadingKey {
    let Some(facts) = self.analyzed.headings().iter().find(|facts| return facts.node == node) else {
      unreachable!("見出しは必ず analyze が headings へ登録している: {node:?}")
    };
    return facts.key;
  }

  /// ラベル宣言を facts から引く（ソース上のラベル名が無いノードは `None`）
  ///
  /// `analyze` は採番した要素のラベルしか登録しないが、「ラベルが付いているのに無採番」という入力は
  /// frontend が弾く（`proof` はそもそも `[label=...]` を受け付けず、数式の
  /// `[numbered=false]` + ラベルは `LabelRequiresNumbering` / `RowLabelNotSupported`）。
  /// よってラベル名があるのに facts に無い、という組み合わせは到達しない。
  fn declared_label(&self, node: NodeId, label: Option<&String>) -> Option<LabelId> {
    label?;
    return self.analyzed.declared_label(node).cloned();
  }

  /// リスト項目を変換する
  fn list_item(&mut self, item: &HirListItem) -> ResolvedListItem {
    return ResolvedListItem {
      content: self.nodes(&item.content),
      marker: item.marker.clone(),
      item_gap: item.item_gap,
    };
  }

  /// 数式の 1 行を変換する
  fn math_row(&mut self, row: &HirMathRow) -> ResolvedMathRow {
    return ResolvedMathRow {
      cells: row.cells.iter().map(|cell| return to_math_nodes(cell)).collect(),
      numbered: row.numbered,
      label: self.declared_label(row.id, row.label.as_ref()),
      // `label_site` が `None`（行末マーカーがない）なら `None` のままにする。
      // `None` は「環境の span をフォールバックに使う」という既存の診断挙動を表す。
      label_span: row.label_site.map(|id| return self.locations.location(id).span),
      counter_value: self.analyzed.counter_value(row.id).cloned(),
    };
  }

  /// 表の 1 行を変換する
  fn table_row(&mut self, row: &HirTableRow) -> ResolvedTableRow {
    return ResolvedTableRow {
      cells: row
        .cells
        .iter()
        .map(|cell| {
          return ResolvedTableCell {
            content: self.inlines(&cell.content),
            span: cell.span,
          };
        })
        .collect(),
      rule_above: row.rule_above,
    };
  }

  /// インラインノード列を変換する
  fn inlines(&mut self, inlines: &[HirInline]) -> Vec<ResolvedInline> {
    return inlines.iter().map(|inline| return self.inline(inline)).collect();
  }

  /// 単一のインラインノードを変換する
  fn inline(&mut self, inline: &HirInline) -> ResolvedInline {
    let span = self.locations.location(inline.id).span;
    return match &inline.kind {
      HirInlineKind::Text(text) => ResolvedInline::Text(text.clone()),
      HirInlineKind::Styled { kind, children } => ResolvedInline::Styled {
        kind: *kind,
        children: self.inlines(children),
      },
      HirInlineKind::Colored { color, children } => ResolvedInline::Colored {
        color: *color,
        children: self.inlines(children),
      },
      HirInlineKind::InlineMath(nodes) => ResolvedInline::InlineMath(to_math_nodes(nodes)),
      HirInlineKind::Symbol(ch) => ResolvedInline::Symbol(*ch),
      HirInlineKind::LineBreak => ResolvedInline::LineBreak,
      HirInlineKind::NoIndent => ResolvedInline::NoIndent,
      HirInlineKind::Ref { .. } => ResolvedInline::Ref {
        target: self.analyzed.reference_target(inline.id).clone(),
        span,
      },
      HirInlineKind::Link { url, children } => ResolvedInline::Link {
        url: url.clone(),
        children: self.inlines(children),
      },
      HirInlineKind::Cite { .. } => ResolvedInline::Cite {
        site: inline.id,
        span,
      },
      HirInlineKind::Footnote { body } => ResolvedInline::Footnote {
        body: self.inlines(body),
        span,
      },
      HirInlineKind::Index { word, reading } => ResolvedInline::Index {
        key: IndexKey {
          word: word.clone(),
          reading: reading.clone(),
        },
        span,
      },
    };
  }
}

/// 書誌（CSL 整形の生成物）を `ResolvedNode` 列へ変換する
///
/// 書誌は `citation::render` が合成する固定形（無採番見出し・アンカー・段落）しか含まない。
/// 見出しには本文の続きとなる `HeadingKey` を 1 つ振り、`headings` の末尾へ足す。
fn bibliography_to_resolved(
  bibliography: &[DocNode],
  next_heading_index: usize,
  headings: &mut Vec<ResolvedHeading>,
) -> Vec<ResolvedNode> {
  let mut result = Vec::with_capacity(bibliography.len());
  let mut heading_index = next_heading_index;
  for node in bibliography {
    match node {
      DocNode::Heading {
        level,
        numbered,
        title,
        span,
        ..
      } => {
        let key = HeadingKey::new(heading_index);
        heading_index += 1;
        let resolved_title = generated_inlines(title);
        headings.push(ResolvedHeading {
          key,
          level: *level,
          // 書誌の見出しは無採番（`citation::render` が `numbered: false` で合成する）。
          counter_value: None,
          title: resolved_title.clone(),
        });
        result.push(ResolvedNode::Heading {
          level: *level,
          numbered: *numbered,
          title: resolved_title,
          label: None,
          key,
          span: *span,
        });
      },
      DocNode::Paragraph(inlines) => result.push(ResolvedNode::Paragraph(generated_inlines(inlines))),
      DocNode::Anchor(target) => result.push(ResolvedNode::Anchor(target.clone())),
      _ => unreachable!("書誌は citation::render が合成する固定形しか含まない: {node:?}"),
    }
  }
  return result;
}

/// 生成物のインライン列（CSL 整形の出力）を `ResolvedInline` 列へ変換する
///
/// 生成物には `\ref` も `\cite` も索引も現れない（`citation::render` が作るのはテキスト・書体・
/// 内部リンクだけ）。到達しない variant は `unreachable!` で落とす。
fn generated_inlines(inlines: &[InlineNode]) -> Vec<ResolvedInline> {
  return inlines.iter().map(generated_inline).collect();
}

/// 生成物のインライン 1 個を変換する
fn generated_inline(inline: &InlineNode) -> ResolvedInline {
  return match inline {
    InlineNode::Text(text) => ResolvedInline::Text(text.clone()),
    InlineNode::Styled { kind, children } => ResolvedInline::Styled {
      kind: *kind,
      children: generated_inlines(children),
    },
    InlineNode::Colored { color, children } => ResolvedInline::Colored {
      color: *color,
      children: generated_inlines(children),
    },
    InlineNode::InternalLink { target, children } => ResolvedInline::InternalLink {
      target: target.clone(),
      children: generated_inlines(children),
    },
    InlineNode::Link { url, children } => ResolvedInline::Link {
      url: url.clone(),
      children: generated_inlines(children),
    },
    InlineNode::Symbol(ch) => ResolvedInline::Symbol(*ch),
    InlineNode::LineBreak => ResolvedInline::LineBreak,
    other => unreachable!("citation::render は生成物にこの variant を作らない: {other:?}"),
  };
}
