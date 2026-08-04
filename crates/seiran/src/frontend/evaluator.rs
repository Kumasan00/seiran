//! 評価器 — CST から Document IR（`DocNode`）を生成する

use crate::{
  frontend::{
    evaluator::command::CommandResult,
    syntax::{
      SyntaxKind,
      ast::CommandView,
      green::{GreenElement, GreenNode},
      token::TokenKind,
    },
  },
  model::{DocNode, InlineNode},
};

pub(crate) mod cite;
mod command;
mod environment;
mod error;
mod inline;
mod math;
mod opt_args;

pub(crate) use environment::lookup_parse_mode as lookup_env_parse_mode;
pub use error::EvalError;

/// CST ノードの子要素を評価して Document IR（`Vec<DocNode>`）に変換する
///
/// 採番とラベル解決は行わない。
///
/// # Errors
///
/// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
pub(crate) fn evaluate_children(source: &str, node: &GreenNode) -> Result<Vec<DocNode>, EvalError> {
  let mut doc_nodes: Vec<DocNode> = Vec::new();
  let mut current_inlines: Vec<InlineNode> = Vec::new();

  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma | TokenKind::Equals => {
          current_inlines.push(InlineNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          current_inlines.push(InlineNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          current_inlines.push(InlineNode::LineBreak);
        },
        TokenKind::ParagraphBreak => {
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
        },
        TokenKind::Underscore => {
          current_inlines.push(InlineNode::Text("_".to_string()));
        },
        TokenKind::Caret => {
          current_inlines.push(InlineNode::Text("^".to_string()));
        },
        TokenKind::Ampersand => {
          current_inlines.push(InlineNode::Text("&".to_string()));
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          let result = command::evaluate_command(&view)?;
          match result {
            CommandResult::Block(block_nodes) => {
              flush_paragraph(&mut doc_nodes, &mut current_inlines);
              doc_nodes.extend(block_nodes);
            },
            CommandResult::Inline(inline_nodes) => {
              current_inlines.extend(inline_nodes);
            },
            CommandResult::NoIndent { span } => {
              // 先行トリビアは許すが、実体のある要素や同じマーカーがあれば段落途中として扱う。
              if current_inlines.iter().any(is_non_blank_inline) {
                return Err(EvalError::NoindentNotAtParagraphStart { span });
              }
              current_inlines.push(InlineNode::NoIndent);
            },
          }
        },
        SyntaxKind::Environment => {
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
          let view = crate::frontend::syntax::ast::EnvironmentView::new(child_node, source);
          let nodes = environment::evaluate_environment(&view)?;
          doc_nodes.extend(nodes);
        },
        SyntaxKind::InlineMath => {
          let math_nodes = math::evaluate_inline_math(source, child_node)?;
          current_inlines.push(InlineNode::InlineMath(math_nodes));
        },
        SyntaxKind::Group => {
          let inner_nodes = evaluate_children(source, child_node)?;
          for doc_node in inner_nodes {
            match doc_node {
              DocNode::Paragraph(inlines) => current_inlines.extend(inlines),
              other => {
                flush_paragraph(&mut doc_nodes, &mut current_inlines);
                doc_nodes.push(other);
              },
            }
          }
        },
        // これらはルート直下に現れない内部ノードである。
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::MathGroup
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {
          unreachable!("トップレベルにはコマンド呼び出し・環境・数式・グループ以外現れない")
        },
      },
    }
  }

  flush_paragraph(&mut doc_nodes, &mut current_inlines);

  return Ok(doc_nodes);
}

/// 蓄積中のインラインノードを `DocNode::Paragraph` としてフラッシュする
///
/// 先頭と末尾の空白は捨てるが、段落内の空白は保持する。
fn flush_paragraph(doc_nodes: &mut Vec<DocNode>, current_inlines: &mut Vec<InlineNode>) {
  let leading_blank = current_inlines.iter().take_while(|inline| return !is_non_blank_inline(inline)).count();
  current_inlines.drain(..leading_blank);
  let trailing_blank = current_inlines.iter().rev().take_while(|inline| return !is_non_blank_inline(inline)).count();
  current_inlines.truncate(current_inlines.len() - trailing_blank);

  if current_inlines.is_empty() {
    return;
  }
  doc_nodes.push(DocNode::Paragraph(std::mem::take(current_inlines)));
  return;
}

/// 段落の先頭判定用に、インライン要素が「実体のある内容」かどうかを返す
fn is_non_blank_inline(inline: &InlineNode) -> bool {
  return match inline {
    InlineNode::Text(text) => !text.trim().is_empty(),
    _ => true,
  };
}
