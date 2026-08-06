//! HIR → 旧 `DocNode` の移行用 adapter（epic #321 Phase 1 限定）
//!
//! 本体経路（citation / resolve / typeset）はすべて HIR と意味 fact を入力に取るようになり
//! （#323 / #324 / #325）、この adapter は `evaluator` の既存テストが `DocNode` で期待値を
//! 書いているための足場としてだけ残っている。テスト側を HIR で書き直した時点で、この module と
//! `evaluator` のテスト専用ヘルパ（`evaluate_children_to_doc_nodes` /
//! `extract_inline_nodes_to_inline_nodes`）をまとめて削除する。
//!
//! 逆方向（`DocNode` → HIR）は提供しない。手組みの `DocNode` に `NodeId` を捏造すると
//! 「すべての `NodeId` は同梱された `HirDocument` が発行したもの」という不変条件を破るため。

use crate::model::{
  DocNode, HirGroup, HirInline, HirInlineKind, HirListItem, HirMath, HirMathKind, HirMathRow, HirNode, HirNodeKind,
  HirProofTarget, HirTableCell, HirTableRow, InlineNode, ListItem, MathNode, MathRow, ProofTarget, SourceMap,
  TableCell, TableRow,
};

/// 1 ソース分の HIR を旧 `DocNode` 列へ変換する
///
/// 位置は `locations` から引いて旧 variant の `span` フィールドへ戻す。authored ノードは
/// 必ず実位置を持つので、この関数は `Span::DUMMY` を生成しない。
pub(crate) fn hir_group_to_doc_nodes(group: &HirGroup, locations: &SourceMap) -> Vec<DocNode> {
  return group.nodes.iter().map(|node| return to_doc_node(node, locations)).collect();
}

/// ブロックノード 1 個を旧 `DocNode` へ変換する
fn to_doc_node(node: &HirNode, locations: &SourceMap) -> DocNode {
  let span = locations.location(node.id).span;
  return match &node.kind {
    // frontend が作る見出しは常に採番対象（`numbered: false` は CSL 整形段が合成する
    // References 見出しだけなので、HIR は採番フラグを持たない）
    HirNodeKind::Heading {
      level,
      title,
      label,
    } => DocNode::Heading {
      level: *level,
      numbered: true,
      title: to_inline_nodes(title, locations),
      label: label.clone(),
      span,
    },
    HirNodeKind::Paragraph(inlines) => DocNode::Paragraph(to_inline_nodes(inlines, locations)),
    HirNodeKind::List {
      ordered,
      items,
      start,
      item_gap,
    } => DocNode::List {
      ordered: *ordered,
      items: items.iter().map(|item| return to_list_item(item, locations)).collect(),
      start: *start,
      item_gap: *item_gap,
    },
    HirNodeKind::MathBlock {
      kind,
      rows,
      numbered,
      label,
    } => DocNode::MathBlock {
      kind: *kind,
      rows: rows.iter().map(|row| return to_math_row(row, locations)).collect(),
      numbered: *numbered,
      label: label.clone(),
      span,
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
    } => DocNode::Figure {
      image_path: image_path.clone(),
      width: *width,
      height: *height,
      dpi: *dpi,
      downsample: *downsample,
      caption: caption.as_ref().map(|inlines| return to_inline_nodes(inlines, locations)),
      caption_position: *caption_position,
      label: label.clone(),
      span,
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
    } => DocNode::Table {
      columns: columns.clone(),
      widths: widths.clone(),
      head: head.iter().map(|row| return to_table_row(row, locations)).collect(),
      rows: rows.iter().map(|row| return to_table_row(row, locations)).collect(),
      caption: caption.as_ref().map(|inlines| return to_inline_nodes(inlines, locations)),
      caption_position: *caption_position,
      label: label.clone(),
      span,
      breakable: *breakable,
    },
    HirNodeKind::Theorem {
      class,
      title,
      body,
      of,
      label,
    } => DocNode::Theorem {
      class: *class,
      title: title.clone(),
      body: body.iter().map(|child| return to_doc_node(child, locations)).collect(),
      of: of.as_ref().map(|target| return to_proof_target(target, locations)),
      label: label.clone(),
      span,
    },
    HirNodeKind::Quote { kind, body } => DocNode::Quote {
      kind: *kind,
      body: body.iter().map(|child| return to_doc_node(child, locations)).collect(),
    },
    HirNodeKind::Rule { width, height } => DocNode::Rule {
      width: *width,
      height: *height,
    },
    HirNodeKind::PageBreak => DocNode::PageBreak,
    HirNodeKind::Space(length) => DocNode::Space(*length),
  };
}

/// リストアイテムを旧 `ListItem` へ変換する
fn to_list_item(item: &HirListItem, locations: &SourceMap) -> ListItem {
  return ListItem {
    content: item.content.iter().map(|node| return to_doc_node(node, locations)).collect(),
    marker: item.marker.clone(),
    item_gap: item.item_gap,
  };
}

/// 表の行を旧 `TableRow` へ変換する
fn to_table_row(row: &HirTableRow, locations: &SourceMap) -> TableRow {
  return TableRow {
    cells: row.cells.iter().map(|cell| return to_table_cell(cell, locations)).collect(),
    rule_above: row.rule_above,
  };
}

/// 表のセルを旧 `TableCell` へ変換する
fn to_table_cell(cell: &HirTableCell, locations: &SourceMap) -> TableCell {
  return TableCell {
    content: to_inline_nodes(&cell.content, locations),
    span: cell.span,
  };
}

/// `proof` の `[of=...]` 参照を旧 `ProofTarget` へ変換する
fn to_proof_target(target: &HirProofTarget, locations: &SourceMap) -> ProofTarget {
  return ProofTarget {
    label: target.label.clone(),
    span: locations.location(target.id).span,
  };
}

/// 数式の行を旧 `MathRow` へ変換する
fn to_math_row(row: &HirMathRow, locations: &SourceMap) -> MathRow {
  return MathRow {
    cells: row.cells.iter().map(|cell| return to_math_nodes(cell, locations)).collect(),
    numbered: row.numbered,
    label: row.label.clone(),
    // `label_site` が `None`（行末マーカーがない）なら旧 `label_span` も `None` のままにする。
    // `None` は「環境の span をフォールバックに使う」という既存の診断挙動を表すため、
    // 行の位置で埋めない。
    label_span: row.label_site.map(|id| return locations.location(id).span),
  };
}

/// インラインノード列を旧 `InlineNode` 列へ変換する
pub(crate) fn to_inline_nodes(inlines: &[HirInline], locations: &SourceMap) -> Vec<InlineNode> {
  return inlines.iter().map(|inline| return to_inline_node(inline, locations)).collect();
}

/// インラインノード 1 個を旧 `InlineNode` へ変換する
fn to_inline_node(inline: &HirInline, locations: &SourceMap) -> InlineNode {
  let span = locations.location(inline.id).span;
  return match &inline.kind {
    HirInlineKind::Text(text) => InlineNode::Text(text.clone()),
    HirInlineKind::Styled { kind, children } => InlineNode::Styled {
      kind: *kind,
      children: to_inline_nodes(children, locations),
    },
    HirInlineKind::Colored { color, children } => InlineNode::Colored {
      color: *color,
      children: to_inline_nodes(children, locations),
    },
    HirInlineKind::InlineMath(nodes) => InlineNode::InlineMath(to_math_nodes(nodes, locations)),
    HirInlineKind::Symbol(ch) => InlineNode::Symbol(*ch),
    HirInlineKind::LineBreak => InlineNode::LineBreak,
    HirInlineKind::NoIndent => InlineNode::NoIndent,
    HirInlineKind::Ref { label } => InlineNode::Ref {
      label: label.clone(),
      span,
    },
    HirInlineKind::Link { url, children } => InlineNode::Link {
      url: url.clone(),
      children: to_inline_nodes(children, locations),
    },
    HirInlineKind::Cite { keys } => InlineNode::Cite {
      keys: keys.clone(),
      node_id: inline.id,
      span,
    },
    HirInlineKind::Footnote { body } => InlineNode::Footnote {
      body: to_inline_nodes(body, locations),
      span,
    },
    HirInlineKind::Index { word, reading } => InlineNode::Index {
      word: word.clone(),
      reading: reading.clone(),
      span,
    },
  };
}

/// 数式ノード列を旧 `MathNode` 列へ変換する
fn to_math_nodes(nodes: &[HirMath], locations: &SourceMap) -> Vec<MathNode> {
  return nodes.iter().map(|node| return to_math_node(node, locations)).collect();
}

/// 数式ノード 1 個を旧 `MathNode` へ変換する
fn to_math_node(node: &HirMath, locations: &SourceMap) -> MathNode {
  return match &node.kind {
    HirMathKind::Text(text) => MathNode::Text(text.clone()),
    HirMathKind::Symbol(ch) => MathNode::Symbol(*ch),
    HirMathKind::Group(children) => MathNode::Group(to_math_nodes(children, locations)),
    HirMathKind::Superscript(child) => MathNode::Superscript(Box::new(to_math_node(child, locations))),
    HirMathKind::Subscript(child) => MathNode::Subscript(Box::new(to_math_node(child, locations))),
    HirMathKind::Frac { numer, denom } => MathNode::Frac {
      numer: Box::new(to_math_node(numer, locations)),
      denom: Box::new(to_math_node(denom, locations)),
    },
    HirMathKind::Sqrt { index, radicand } => MathNode::Sqrt {
      index: index.as_ref().map(|node| return Box::new(to_math_node(node, locations))),
      radicand: Box::new(to_math_node(radicand, locations)),
    },
    HirMathKind::Styled { style, body } => MathNode::Styled {
      style: *style,
      body: to_math_nodes(body, locations),
    },
  };
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use crate::model::{DocNode, HirInlineKind, HirNode, HirNodeKind, InlineNode, NodeId};

  /// ブロックノード列（段落 1 つ想定）から最初の `\cite` の HIR `NodeId` を探す
  fn find_first_cite_id(nodes: &[HirNode]) -> Option<NodeId> {
    for node in nodes {
      if let HirNodeKind::Paragraph(inlines) = &node.kind {
        for inline in inlines {
          if let HirInlineKind::Cite { .. } = &inline.kind {
            return Some(inline.id);
          }
        }
      }
    }
    return None;
  }

  /// `DocNode` 列（段落 1 つ想定）から最初の `Cite` を探す
  fn find_first_doc_cite(nodes: &[DocNode]) -> Option<&InlineNode> {
    for node in nodes {
      if let DocNode::Paragraph(inlines) = node {
        for inline in inlines {
          if let InlineNode::Cite { .. } = inline {
            return Some(inline);
          }
        }
      }
    }
    return None;
  }

  #[test]
  fn cite_carries_hir_node_id() {
    // Arrange — `\cite` を 1 つ含むソースをパースする
    let hir = crate::frontend::parse_source(r"本文 \cite{rika} です。", crate::model::SourceId::new(0))
      .expect("パースに成功するはず");
    let hir_cite_id = find_first_cite_id(&hir.group.nodes).expect("HIR に Cite があるはず");
    let document = crate::model::HirDocument::assemble(vec![hir]);
    let group = document.groups().first().expect("1 グループあるはず");

    // Act
    let nodes = super::hir_group_to_doc_nodes(group, document.locations());

    // Assert
    let InlineNode::Cite { node_id, .. } = find_first_doc_cite(&nodes).expect("DocNode に Cite があるはず")
    else {
      unreachable!("find_first_doc_cite は Cite だけを返す")
    };
    assert_eq!(*node_id, hir_cite_id, "adapter は HIR の NodeId をそのまま運ぶはず");
  }
}
