//! インライン要素抽出のヘルパー
//!
//! `extract_inline_nodes` は CST のサブツリーをインライン文脈として歩き、
//! `InlineNode` 列に変換する。テキスト装飾コマンド（`\textbf` 等）と
//! 単一文字コマンド（`\alpha` 等）の解釈は [`crate::evaluator::command::COMMAND_MAP`]
//! を **唯一のソース** として参照する（ハードコードした name match を持たない）。
//!
//! `resolve_symbol_command` はコマンド名から単一 Unicode 文字を返す純粋関数で、
//! 数式ノード変換 (`Evaluator::evaluate_math_command`) からも参照される。

use syntax::{
  ast::CommandView,
  green::{GreenElement, GreenNode},
  kind::SyntaxKind,
  token::TokenKind,
};

use crate::{
  document::InlineNode,
  evaluator::{
    Evaluator,
    command::{COMMAND_MAP, CommandKind},
  },
};

/// `GreenNode` の子要素から `InlineNode` のリストを構築する
///
/// 見出しの引数など、テキストノードとコマンドを `InlineNode` に変換します。
/// インラインコマンド（`\textbf`, `\emph` 等）と単一文字コマンドの解釈は
/// [`COMMAND_MAP`] を **唯一のソース** として参照する。
/// `InlineMath` ノードは `InlineNode::InlineMath` に変換します。
pub(crate) fn extract_inline_nodes(source: &str, node: &GreenNode) -> Vec<InlineNode> {
  let mut inlines = Vec::new();
  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma | TokenKind::Equals => {
          inlines.push(InlineNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          inlines.push(InlineNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          inlines.push(InlineNode::LineBreak);
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          match COMMAND_MAP.get(view.name()).copied() {
            Some(CommandKind::InlineWrapper(wrapper)) => {
              if let Some(arg) = view.first_arg() {
                let children = extract_inline_nodes(source, arg);
                inlines.push(wrapper(children));
              }
            },
            Some(CommandKind::SingleChar(ch)) => {
              inlines.push(InlineNode::Symbol(ch));
            },
            _ => {
              // 見出しの引数などインライン文脈に出現しない種類（Headline / Space / Undefined）は
              // 黙って無視。エラーは `Evaluator::evaluate_command` 側で扱う。
            },
          }
        },
        SyntaxKind::InlineMath => {
          let math_nodes = Evaluator::evaluate_inline_math(source, child_node);
          inlines.push(InlineNode::InlineMath(math_nodes));
        },
        SyntaxKind::Group => {
          // グループの中身を再帰的に処理
          let children = extract_inline_nodes(source, child_node);
          inlines.extend(children);
        },
        _ => {},
      },
    }
  }
  return inlines;
}

/// コマンド名からシンボル文字を解決する
///
/// ギリシャ文字・数学記号等の引数なしコマンドを対応する Unicode 文字に変換します。
/// 未知のコマンド名、または `SingleChar` 以外のコマンド種別の場合は `None` を返します。
///
/// 解決の単一ソースは [`COMMAND_MAP`]。コマンド追加はそちらだけを編集すれば、
/// 本関数および `Evaluator::evaluate_command` の双方に反映される。
#[must_use]
pub(crate) fn resolve_symbol_command(name: &str) -> Option<char> {
  if let Some(CommandKind::SingleChar(ch)) = COMMAND_MAP.get(name).copied() {
    return Some(ch);
  }
  return None;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::parse;

  use super::*;

  #[test]
  fn extract_inline_nodes_with_textbf() {
    let arena = Bump::new();
    let source = "\\section{\\textbf{太字タイトル}}";
    let cst = parse(source, &arena).unwrap();
    // Root > CommandCall(\section) > MandatoryArg > CommandCall(\textbf)
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], InlineNode::Strong(_)));
  }

  #[test]
  fn extract_inline_nodes_with_symbol_command() {
    let arena = Bump::new();
    let source = "\\section{\\alpha}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], InlineNode::Symbol('α')));
  }

  #[test]
  fn extract_inline_nodes_with_inline_math() {
    let arena = Bump::new();
    let source = "\\section{数式 $x^2$ です}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    // Text("数式"), Text(" "), InlineMath(...), Text(" "), Text("です")
    let has_math = inlines.iter().any(|n| matches!(n, InlineNode::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_mixed_text_and_commands() {
    let arena = Bump::new();
    let source = "\\section{Hello \\textbf{World}}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    // Text("Hello"), Text(" "), Strong(...)
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0], InlineNode::Text(t) if t == "Hello"));
    assert!(matches!(&inlines[1], InlineNode::Text(t) if t == " "));
    assert!(matches!(&inlines[2], InlineNode::Strong(_)));
  }
}
