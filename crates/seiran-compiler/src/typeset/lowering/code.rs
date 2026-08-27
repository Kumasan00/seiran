//! コード（`document::HirNodeKind::CodeBlock` / `document::HirInlineKind::Code`）の lowering
//!
//! 内容としてのコードなので、空白・字下げ・改行はソースのまま組む。1 行を
//! [`LayoutNode::TextAtom`]（伸縮しない閉じた箱）1 つに落とし、行と行の間に
//! [`LayoutNode::LineBreak`] を挟むことで、行揃えでも字下げが動かないようにする。

use crate::{
  color::Color,
  document::FontKind,
  length::Length,
  typeset::lowering::{
    LoweringContext,
    layout_node::{LayoutNode, TextStyle},
    paragraph,
  },
};

/// コードのテキストスタイル（等幅・本文サイズ）を返す
///
/// フォントは config.toml の既存 monospace スロット。書体・サイズの style.toml 設定
/// （`[code]` セクション）はハイライト段の issue のスコープ。
fn code_text_style(font_size: Length, color: Option<Color>) -> TextStyle {
  return TextStyle {
    font_size,
    font_kind: FontKind::Monospace,
    color,
  };
}

/// コードブロック（`code` 環境）をレイアウトノードに変換する
///
/// 段落 1 つとして組み、行区切りは強制改行にする。行が `Block::Paragraph` の行として
/// 出るので、長いコードブロックでも行単位でページ分割できる。字下げ（`first_line_indent`）は
/// 抑止する — コードの 1 桁目はソースの 1 桁目でなければならない。
pub(super) fn lower_code_block(ctx: &LoweringContext<'_>, text: &str) -> Vec<LayoutNode> {
  let style = code_text_style(ctx.default_font_size(), None);
  let mut content = Vec::new();
  for (index, line) in text.split('\n').enumerate() {
    if index > 0 {
      content.push(LayoutNode::LineBreak);
    }
    content.push(LayoutNode::TextAtom(line.to_string(), style));
  }
  return paragraph::assemble_paragraph(ctx, content, true);
}

/// インラインコード（`\code{...}`）をレイアウトノードに変換する
///
/// 書体は等幅に差し替え、サイズと色は周囲から継承する（`\color{... \code{x} ...}` は効く）。
/// 内容に改行があっても行を割らず、シェーピング段（`typeset::boxing`）が空白へ畳む。
pub(super) fn lower_inline_code(text: &str, parent_style: TextStyle) -> Vec<LayoutNode> {
  return vec![LayoutNode::TextAtom(
    text.to_string(),
    code_text_style(parent_style.font_size, parent_style.color),
  )];
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    style::Style as ReadStyle,
    typeset::lowering::test_support::{analyzed, lower},
  };

  /// `.sei` ソースを lower して `TextAtom` のテキストだけを並べる
  fn atom_texts(nodes: &[LayoutNode]) -> Vec<&str> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::TextAtom(text, _) => return Some(text.as_str()),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn code_block_lowers_each_line_to_one_atom_separated_by_line_breaks() {
    // Arrange
    let style = ReadStyle::default();
    let source = "\\begin{code}\nfn main() {\n    let x = 1;\n}\n\\end{code}\n";

    // Act
    let nodes = lower(&style, &analyzed(source));

    // Assert
    assert_eq!(atom_texts(&nodes), vec!["fn main() {", "    let x = 1;", "}"]);
    let breaks = nodes.iter().filter(|n| matches!(n, LayoutNode::LineBreak)).count();
    assert_eq!(breaks, 2, "行の間だけに強制改行が入る: {nodes:?}");
  }

  #[test]
  fn code_block_keeps_blank_line_as_an_empty_atom() {
    // Arrange
    let style = ReadStyle::default();
    let source = "\\begin{code}\na\n\nb\n\\end{code}\n";

    // Act
    let nodes = lower(&style, &analyzed(source));

    // Assert
    assert_eq!(atom_texts(&nodes), vec!["a", "", "b"]);
  }

  #[test]
  fn code_block_uses_monospace_and_suppresses_first_line_indent() {
    // Arrange
    let mut style = ReadStyle::default();
    style.text.first_line_indent = Length::pt(15.0);
    let source = "\\begin{code}\nx\n\\end{code}\n";

    // Act
    let nodes = lower(&style, &analyzed(source));

    // Assert
    let LayoutNode::TextAtom(_, text_style) = &nodes[0] else {
      panic!("先頭は TextAtom であるべき: {nodes:?}");
    };
    assert_eq!(text_style.font_kind, FontKind::Monospace);
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Kern { .. })), "字下げ Kern は出ない: {nodes:?}");
  }

  #[test]
  fn inline_code_becomes_a_monospace_atom_in_the_paragraph() {
    // Arrange
    let style = ReadStyle::default();
    let source = "前 \\code{if x { y }} 後\n";

    // Act
    let nodes = lower(&style, &analyzed(source));

    // Assert
    assert_eq!(atom_texts(&nodes), vec!["if x { y }"]);
    let LayoutNode::TextAtom(_, text_style) =
      nodes.iter().find(|n| matches!(n, LayoutNode::TextAtom(..))).expect("TextAtom があるはず")
    else {
      unreachable!("find が TextAtom だけを返す")
    };
    assert_eq!(text_style.font_kind, FontKind::Monospace);
    assert_eq!(text_style.font_size, style.text.font_size, "サイズは周囲から継承する");
  }
}
