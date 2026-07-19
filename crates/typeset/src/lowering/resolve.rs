//! pass2: `\ref` 解決 — `LayoutNode` ツリーを歩いて `LayoutNode::Ref` を埋める
//!
//! pass1（[`super::lower_nodes`] の主走査）が発行した `LayoutNode::Ref` プレースホルダを、
//! pass1 完了時点で確定している [`super::counter::CounterRegistry`] の labels テーブルで解決する。
//! `\ref` は前方参照（本文より後ろで定義されるラベルを指す）を許すため、pass1 とは別の
//! 走査として全体完了後にもう一度木を歩く必要がある。

use model::LinkTarget;

use super::{
  LoweringError,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TableLayout, merge_adjacent_text_at},
};

/// `Vec<LayoutNode>` を再帰的に走査して `LayoutNode::Ref` を `CounterRegistry` で解決する
///
/// 見つかった `Ref { label, span, style, as_link }` は `registry.resolve_label(label)` の結果で
/// `as_link` が `true`（`\ref`）なら `LayoutNode::Link { target: Internal(label),
/// children: [Text(resolved, style)] }` に、`false`（`proof` の `{of}`）なら
/// `LayoutNode::Text(resolved, style)` 単体に置き換える。
///
/// `{of}` 解決後は隣接する同一スタイルの `Text` と結合する（[`super::template::expand_template`] が
/// 元々プレーン文字列として埋め込んでいたときと同じシェーピング結果にするため）。このマージは
/// **今まさに `{of}` を解決した箇所の前後のみ**に限定する（`as_link: true` で `Link` に解決した場合や、
/// 数式のように意図的に個別 `Text` ノードのまま保つ既存の隣接ペアには一切触れない）。
///
/// # Errors
///
/// 未定義ラベルが見つかった場合に [`LoweringError::UnresolvedReference`] を返します。
pub(crate) fn resolve_refs(nodes: &mut Vec<LayoutNode>, registry: &CounterRegistry) -> Result<(), LoweringError> {
  let mut resolved_positions: Vec<usize> = Vec::new();
  for (i, node) in nodes.iter_mut().enumerate() {
    if resolve_node(node, registry)? {
      resolved_positions.push(i);
    }
  }
  merge_adjacent_text_at(nodes, &resolved_positions);
  return Ok(());
}

/// 単一ノードを解決する。戻り値は「`{of}` 解決で `Text` に置き換わったか」（マージ判定に使う）
fn resolve_node(node: &mut LayoutNode, registry: &CounterRegistry) -> Result<bool, LoweringError> {
  match node {
    LayoutNode::Ref {
      label,
      span,
      style,
      as_link,
      source,
    } => {
      let Some(resolved) = registry.resolve_label(label) else {
        return Err(LoweringError::UnresolvedReference {
          label: label.clone(),
          span: *span,
          source_id: *source,
        });
      };
      let text = LayoutNode::Text(resolved.to_string(), *style);
      let resolved_as_text = !*as_link;
      *node = if *as_link {
        LayoutNode::Link {
          target: LinkTarget::Internal(label.clone()),
          children: vec![text],
        }
      } else {
        text
      };
      return Ok(resolved_as_text);
    },
    LayoutNode::VBox { children, .. }
    | LayoutNode::HBox { children, .. }
    | LayoutNode::Raise { children, .. }
    | LayoutNode::Link { children, .. }
    | LayoutNode::Footnote { body: children, .. } => {
      resolve_refs(children, registry)?;
    },
    LayoutNode::FlushRight(children) => resolve_refs(children, registry)?,
    LayoutNode::Table(table) => resolve_table(table, registry)?,
    LayoutNode::MathBlock {
      rows, env_number, ..
    } => {
      for row in rows {
        for cell in &mut row.cells {
          resolve_refs(cell, registry)?;
        }
        if let Some(number) = &mut row.number {
          resolve_refs(number, registry)?;
        }
      }
      if let Some(number) = env_number {
        resolve_refs(number, registry)?;
      }
    },
    LayoutNode::Text(..)
    | LayoutNode::Rule { .. }
    | LayoutNode::Image { .. }
    | LayoutNode::Glue { .. }
    | LayoutNode::Kern { .. }
    | LayoutNode::Vkern { .. }
    | LayoutNode::Anchor(_)
    | LayoutNode::LineBreak
    | LayoutNode::PageBreak
    | LayoutNode::KeepWithNext
    | LayoutNode::IndexMark { .. } => {},
  }
  return Ok(false);
}

/// テーブルの各セルの内容を再帰的に走査して `\ref` を解決する（[`resolve_refs`] のテーブル専用エントリ）
///
/// `table.head` と `table.rows` の全セルの `content` に対して [`resolve_refs`] を適用する。
///
/// # Errors
///
/// 未定義ラベルが見つかった場合に [`LoweringError::UnresolvedReference`] を返します。
fn resolve_table(table: &mut TableLayout, registry: &CounterRegistry) -> Result<(), LoweringError> {
  for row in table.head.iter_mut().chain(table.rows.iter_mut()) {
    for cell in &mut row.cells {
      resolve_refs(&mut cell.content, registry)?;
    }
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use config::{CounterName, Style};
  use miette::SourceSpan;
  use model::{FontKind, Length};

  use super::{
    super::layout_node::{TableCellLayout, TableRowLayout, TextStyle},
    *,
  };

  fn dummy_span() -> SourceSpan { return SourceSpan::from((0_usize, 0_usize)); }

  fn style() -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      font_kind: FontKind::Serif,
      color: None,
    };
  }

  fn ref_node(label: &str) -> LayoutNode {
    return LayoutNode::Ref {
      label: label.to_string(),
      span: dummy_span(),
      style: style(),
      as_link: true,
      source: super::super::SourceId::new(0),
    };
  }

  #[test]
  fn resolve_refs_rewrites_ref_to_link() {
    // Arrange — chapter を 1 進めて section にラベルを登録した registry
    let mut registry = CounterRegistry::from_style(&Style::default());
    registry.increment(CounterName::Chapter);
    let number = registry.increment(CounterName::Section);
    assert!(registry.register_label("sec:intro", CounterName::Section, &number));
    let mut nodes = vec![LayoutNode::VBox {
      children: vec![ref_node("sec:intro")],
      margin_bottom: Length::ZERO,
      indent: Length::ZERO,
      right_indent: Length::ZERO,
      align: model::Align::Left,
    }];

    // Act
    resolve_refs(&mut nodes, &registry).unwrap();

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[0] else {
      panic!("VBox が期待されます");
    };
    let LayoutNode::Link { target, children } = &children[0] else {
      panic!("Link が期待されます: {:?}", children[0]);
    };
    assert_eq!(*target, model::LinkTarget::Internal("sec:intro".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "Section 1.1"));
  }

  #[test]
  fn resolve_refs_returns_unresolved_reference_for_missing_label() {
    // Arrange — ラベル未登録の状態で Ref を解決
    let registry = CounterRegistry::default_for_seiran();
    let mut nodes = vec![ref_node("nope")];

    // Act
    let result = resolve_refs(&mut nodes, &registry);

    // Assert
    assert!(matches!(result, Err(LoweringError::UnresolvedReference { ref label, .. }) if label == "nope"));
  }

  #[test]
  fn resolve_refs_recurses_into_table_cells() {
    // Arrange
    let mut registry = CounterRegistry::default_for_seiran();
    registry.increment(CounterName::Chapter);
    assert!(registry.register_label("ch:intro", CounterName::Chapter, "1"));
    let mut nodes = vec![LayoutNode::Table(TableLayout {
      columns: vec![],
      head: vec![],
      rows: vec![TableRowLayout {
        cells: vec![TableCellLayout {
          content: vec![ref_node("ch:intro")],
          span: 1,
        }],
        rule_above: false,
      }],
      breakable: true,
    })];

    // Act
    resolve_refs(&mut nodes, &registry).unwrap();

    // Assert
    let LayoutNode::Table(table) = &nodes[0] else {
      panic!("Table が期待されます");
    };
    assert!(matches!(&table.rows[0].cells[0].content[0], LayoutNode::Link { .. }));
  }
}
