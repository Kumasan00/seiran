//! コード（`document::HirNodeKind::CodeBlock` / `document::HirInlineKind::Code`）の lowering
//!
//! 内容としてのコードなので、空白・字下げ・改行はソースのまま組む。1 行を
//! [`InlineNode::TextAtom`]（伸縮しない閉じた箱）1 つに落とし、行と行の間に
//! [`InlineNode::LineBreak`] を挟むことで、行揃えでも字下げが動かないようにする。

use crate::{
  color::Color,
  document::{TextAlignment, Typeface},
  length::Length,
  typeset::lowering::{
    LoweringContext,
    layout_node::{InlineNode, LayoutNode, TextStyle},
    paragraph,
  },
};

/// コードのテキストスタイル（等幅）を返す
fn code_text_style(font_size: Length, color: Option<Color>) -> TextStyle {
  return TextStyle {
    font_size,
    typeface: Typeface::Monospace,
    color,
    script_level: None,
  };
}

/// コードブロック（`code` 環境）をレイアウトノードに変換する
///
/// 行が `Block::Paragraph` の行として出るので、長いコードブロックでも行単位でページ分割できる。
/// 字下げ（`first_line_indent`）は抑止する — コードの 1 桁目はソースの 1 桁目でなければならない。
/// 揃えは `[text].alignment` に従わず左に固定する — 行どうしの桁の相対位置が内容なので、行ごとに寄せると字下げが崩れる。
pub(super) fn lower_code_block(ctx: &LoweringContext<'_>, text: &str) -> Vec<LayoutNode> {
  let style = code_text_style(ctx.default_font_size(), None);
  let mut content = Vec::new();
  for (index, line) in text.split('\n').enumerate() {
    if index > 0 {
      content.push(InlineNode::LineBreak);
    }
    content.push(InlineNode::TextAtom(line.to_string(), style));
  }
  return vec![LayoutNode::VBox {
    children: paragraph::assemble_paragraph(ctx, content, true),
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    alignment: Some(TextAlignment::Left),
  }];
}

/// インラインコード（`\code{...}`）をインラインノードに変換する
///
/// 書体は等幅に差し替え、サイズと色は周囲から継承する（`\color{... \code{x} ...}` は効く）。
/// 内容に改行があっても行を割らず、シェーピング段（`typeset::boxing`）が空白へ畳む。
pub(super) fn lower_inline_code(text: &str, parent_style: TextStyle) -> Vec<InlineNode> {
  return vec![InlineNode::TextAtom(
    text.to_string(),
    code_text_style(parent_style.font_size, parent_style.color),
  )];
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::TextAlignment,
    style::Style as ReadStyle,
    typeset::lowering::test_support::{analyzed, lower},
  };

  /// コードブロックの `VBox` の子要素列を取り出す
  fn code_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    let [LayoutNode::VBox { children, .. }] = nodes else {
      panic!("コードブロックは VBox 1 個で出るはず: {nodes:?}");
    };
    return children;
  }

  /// レイアウトノード列から `TextAtom` のテキストだけを並べる
  fn atom_texts(nodes: &[LayoutNode]) -> Vec<&str> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::Inline(InlineNode::TextAtom(text, _)) => return Some(text.as_str()),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn code_block_lowers_each_line_to_one_atom_separated_by_line_breaks() {
    let style = ReadStyle::default();
    let source = "\\begin{code}\nfn main() {\n    let x = 1;\n}\n\\end{code}\n";

    let nodes = lower(&style, &analyzed(source));

    let children = code_children(&nodes);
    assert_eq!(atom_texts(children), vec!["fn main() {", "    let x = 1;", "}"]);
    let breaks = children.iter().filter(|n| matches!(n, LayoutNode::Inline(InlineNode::LineBreak))).count();
    assert_eq!(breaks, 2, "行の間だけに強制改行が入る: {nodes:?}");
  }

  #[test]
  fn code_block_keeps_blank_line_as_an_empty_atom() {
    let style = ReadStyle::default();
    let source = "\\begin{code}\na\n\nb\n\\end{code}\n";

    let nodes = lower(&style, &analyzed(source));

    assert_eq!(atom_texts(code_children(&nodes)), vec!["a", "", "b"]);
  }

  #[test]
  fn code_block_uses_monospace_and_suppresses_first_line_indent() {
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(15.0);
    let source = "\\begin{code}\nx\n\\end{code}\n";

    let nodes = lower(&style, &analyzed(source));

    let children = code_children(&nodes);
    let LayoutNode::Inline(InlineNode::TextAtom(_, text_style)) = &children[0] else {
      panic!("先頭は TextAtom であるべき: {nodes:?}");
    };
    assert_eq!(text_style.typeface, Typeface::Monospace);
    assert!(
      !children.iter().any(|n| matches!(n, LayoutNode::Inline(InlineNode::Kern { .. }))),
      "字下げ Kern は出ない: {nodes:?}"
    );
  }

  #[test]
  fn code_block_is_left_aligned_regardless_of_text_alignment() {
    let mut style = ReadStyle::default();
    style.text.alignment = TextAlignment::Center;

    let nodes = lower(&style, &analyzed("\\begin{code}\nx\n\\end{code}\n"));

    let [LayoutNode::VBox { alignment, .. }] = nodes.as_slice() else {
      panic!("コードブロックは VBox 1 個で出るはず: {nodes:?}");
    };
    assert_eq!(*alignment, Some(TextAlignment::Left), "行どうしの桁の相対位置を保つため左固定");
  }

  #[test]
  fn inline_code_becomes_a_monospace_atom_in_the_paragraph() {
    let style = ReadStyle::default();
    let source = "前 \\code{if x { y }} 後\n";

    let nodes = lower(&style, &analyzed(source));

    assert_eq!(atom_texts(&nodes), vec!["if x { y }"]);
    let LayoutNode::Inline(InlineNode::TextAtom(_, text_style)) = nodes
      .iter()
      .find(|n| matches!(n, LayoutNode::Inline(InlineNode::TextAtom(..))))
      .expect("TextAtom があるはず")
    else {
      unreachable!("find が TextAtom だけを返す")
    };
    assert_eq!(text_style.typeface, Typeface::Monospace);
    assert_eq!(text_style.font_size, style.text.font_size, "サイズは周囲から継承する");
  }
}
