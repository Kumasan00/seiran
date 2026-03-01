//! CST → AST ローワリング
#![allow(unused_assignments)] // thiserror 生成コードの false positive 抑制
//! ロスレスな CST（具象構文木）から、型付き AST（抽象構文木）へ変換します。
//! この過程でコメント・空白トークンを除去し、意味的な構造に変換します。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! CST (syntax::SyntaxNode)
//!   ↓ [ast_lower (このモジュール)]
//! AST (ast::Block)
//! ```

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  ast::{Block, Command, Environment, InlineMathBlock, InlineMathNode, InlineMathNodeKind, Node, NodeKind},
  syntax::{SyntaxElement, SyntaxKind, SyntaxNode},
  token::TokenKind,
};

// =============================================================================
// エラー型
// =============================================================================

/// AST ローワリング時のエラー
///
/// CST から AST への変換中に発生するエラーを表現します。
/// `#[label]` によるソース位置情報を持ち、`miette::NamedSource` と
/// 組み合わせることでソースコード付きのエラー表示が可能です。
#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code, unused_assignments)]
pub enum LowerError {
  /// 予期しない CST ノードが出現した場合
  #[error("AST 変換中に予期しないノードです: {kind:?}")]
  #[diagnostic(code(parser::lower::unexpected_node), help("CST の構造に問題がある可能性があります"))]
  UnexpectedNode {
    /// ノードの種別
    kind: SyntaxKind,
    /// ソース上の位置
    #[label("このノードは AST 変換で処理できません")]
    span: SourceSpan,
  },
}

// =============================================================================
// ローワリング実装
// =============================================================================

/// CST ルートノードを AST ブロックに変換する
///
/// # Arguments
///
/// * `source` - 元のソーステキスト
/// * `root` - CST のルートノード
///
/// # Returns
///
/// AST のノードリスト（ブロック）
///
/// # Errors
///
/// CST の構造が不正な場合
pub(crate) fn lower<'a>(source: &'a str, root: &SyntaxNode) -> Result<Block<'a>, LowerError> {
  return lower_children(source, &root.children);
}

/// 子要素のリストを AST ブロックに変換する
fn lower_children<'a>(source: &'a str, children: &[SyntaxElement]) -> Result<Block<'a>, LowerError> {
  let mut nodes = Vec::new();

  for child in children {
    match child {
      SyntaxElement::Token(token) => {
        match token.kind {
          TokenKind::Text => {
            nodes.push(Node::new(NodeKind::Text(token.text(source)), token.span));
          },
          TokenKind::Escaped => {
            // エスケープ文字：ソースの部分文字列（バックスラッシュ後の文字）を参照
            let text = &source[token.span.start as usize + 1..token.span.end as usize];
            nodes.push(Node::new(NodeKind::Text(text), token.span));
          },
          TokenKind::LineBreak => {
            nodes.push(Node::new(NodeKind::LineBreak, token.span));
          },
          TokenKind::ParagraphBreak => {
            nodes.push(Node::new(NodeKind::ParagraphBreak, token.span));
          },
          // コメント・空白・構造文字・単独コマンドトークンは AST では無視
          TokenKind::Comment
          | TokenKind::LBrace
          | TokenKind::RBrace
          | TokenKind::LBracket
          | TokenKind::RBracket
          | TokenKind::Dollar
          | TokenKind::Unknown
          | TokenKind::Command => {},
        }
      },
      SyntaxElement::Node(node) => {
        match node.kind {
          SyntaxKind::CommandCall => {
            let cmd = lower_command(source, node)?;
            nodes.push(Node::new(NodeKind::Command(cmd), node.span));
          },
          SyntaxKind::Environment => {
            let env = lower_environment(source, node)?;
            nodes.push(Node::new(NodeKind::Environment(env), node.span));
          },
          SyntaxKind::InlineMath => {
            let math = lower_inline_math(source, node)?;
            nodes.push(Node::new(NodeKind::InlineMath(math), node.span));
          },
          SyntaxKind::Group => {
            // グループの中身をフラットに展開（現行動作と同じ）
            let inner = lower_arg_contents(source, node)?;
            nodes.extend(inner);
          },
          _ => {
            // その他のノード種別は無視
          },
        }
      },
    }
  }

  return Ok(nodes);
}

/// `CommandCall` ノードを `AST Command` に変換する
fn lower_command<'a>(source: &'a str, node: &SyntaxNode) -> Result<Command<'a>, LowerError> {
  let mut name = "";
  let mut args: Vec<Block<'a>> = Vec::new();
  let mut opt_args: Vec<Block<'a>> = Vec::new();
  let mut cmd_span = node.span;

  for child in &node.children {
    match child {
      SyntaxElement::Token(token) if token.kind == TokenKind::Command => {
        name = token.command_name(source);
        cmd_span = token.span;
      },
      SyntaxElement::Node(n) if n.kind == SyntaxKind::MandatoryArg => {
        let block = lower_arg_contents(source, n)?;
        args.push(block);
      },
      SyntaxElement::Node(n) if n.kind == SyntaxKind::OptArg => {
        let block = lower_arg_contents(source, n)?;
        opt_args.push(block);
      },
      _ => {}, // コメントトークン等は無視
    }
  }

  return Ok(Command {
    name,
    args,
    opt_args,
    span: cmd_span,
  });
}

/// Environment ノードを AST Environment に変換する
fn lower_environment<'a>(source: &'a str, node: &SyntaxNode) -> Result<Environment<'a>, LowerError> {
  let mut name = "";
  let mut args: Vec<Block<'a>> = Vec::new();
  let mut opt_args: Vec<Block<'a>> = Vec::new();
  let mut body: Block<'a> = Vec::new();

  for child in node.child_nodes() {
    match child.kind {
      SyntaxKind::EnvironmentBegin => {
        // 環境名と引数を取得
        for begin_child in &child.children {
          match begin_child {
            SyntaxElement::Token(t) if t.kind == TokenKind::Command => {
              // \begin トークン → 無視（名前は MandatoryArg から取得）
            },
            SyntaxElement::Node(n) if n.kind == SyntaxKind::MandatoryArg => {
              if name.is_empty() {
                // 最初の MandatoryArg は環境名
                name = extract_text_from_arg(source, n);
              } else {
                // 追加の MandatoryArg は環境引数
                let block = lower_arg_contents(source, n)?;
                args.push(block);
              }
            },
            SyntaxElement::Node(n) if n.kind == SyntaxKind::OptArg => {
              let block = lower_arg_contents(source, n)?;
              opt_args.push(block);
            },
            _ => {},
          }
        }
      },
      SyntaxKind::EnvironmentBody => {
        body = lower_children(source, &child.children)?;
      },
      _ => {},
    }
  }

  return Ok(Environment {
    name,
    args,
    opt_args,
    children: body,
    span: node.span,
  });
}

/// 引数ノード（`MandatoryArg` / `OptArg` / `Group`）の中身を AST ブロックに変換する
///
/// 括弧トークン（`{`, `}`, `[`, `]`）は除外し、中身のみを返す
fn lower_arg_contents<'a>(source: &'a str, node: &SyntaxNode) -> Result<Block<'a>, LowerError> {
  let mut inner_children = Vec::new();
  for child in &node.children {
    match child {
      SyntaxElement::Token(t)
        if matches!(t.kind, TokenKind::LBrace | TokenKind::RBrace | TokenKind::LBracket | TokenKind::RBracket) =>
      {
        // 括弧トークンをスキップ
      },
      _ => {
        inner_children.push(child.clone());
      },
    }
  }
  return lower_children(source, &inner_children);
}

/// `InlineMath` ノードを AST `InlineMathBlock` に変換する
fn lower_inline_math<'a>(source: &'a str, node: &SyntaxNode) -> Result<InlineMathBlock<'a>, LowerError> {
  let mut math_nodes = Vec::new();

  for child in &node.children {
    match child {
      SyntaxElement::Token(token) => {
        match token.kind {
          // $ は数式の区切り — AST では不要
          TokenKind::Text => {
            math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Text(token.text(source)), token.span));
          },
          TokenKind::Escaped => {
            let text = &source[token.span.start as usize + 1..token.span.end as usize];
            math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Text(text), token.span));
          },
          _ => {}, // その他のトークンは無視
        }
      },
      SyntaxElement::Node(n) => match n.kind {
        SyntaxKind::CommandCall => {
          let cmd = lower_command(source, n)?;
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Command(cmd), n.span));
        },
        SyntaxKind::MathGroup => {
          let group = lower_math_group(source, n)?;
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Group(group), n.span));
        },
        _ => {},
      },
    }
  }

  return Ok(math_nodes);
}

/// `MathGroup` ノードを AST `InlineMathBlock` に変換する
fn lower_math_group<'a>(source: &'a str, node: &SyntaxNode) -> Result<InlineMathBlock<'a>, LowerError> {
  let mut math_nodes = Vec::new();

  for child in &node.children {
    match child {
      SyntaxElement::Token(token) => match token.kind {
        TokenKind::Text => {
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Text(token.text(source)), token.span));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Text(text), token.span));
        },
        _ => {},
      },
      SyntaxElement::Node(n) => match n.kind {
        SyntaxKind::CommandCall => {
          let cmd = lower_command(source, n)?;
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Command(cmd), n.span));
        },
        SyntaxKind::MathGroup => {
          let group = lower_math_group(source, n)?;
          math_nodes.push(InlineMathNode::new(InlineMathNodeKind::Group(group), n.span));
        },
        _ => {},
      },
    }
  }

  return Ok(math_nodes);
}

/// 引数ノードからテキスト内容を抽出する（環境名取得用）
fn extract_text_from_arg<'a>(source: &'a str, arg_node: &SyntaxNode) -> &'a str {
  for child in &arg_node.children {
    if let SyntaxElement::Token(t) = child
      && t.kind == TokenKind::Text
    {
      return t.text(source);
    }
  }
  return "";
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use crate::parser::parser;

  /// ソースをパースして AST に変換するヘルパー
  fn parse_and_lower(source: &str) -> Block<'_> {
    let cst = parser(source).unwrap();
    return lower(source, &cst).unwrap();
  }

  #[test]
  fn empty_input() {
    let block = parse_and_lower("");
    assert!(block.is_empty());
    return;
  }

  #[test]
  fn plain_text_becomes_text_node() {
    let block = parse_and_lower("hello world");
    assert_eq!(block.len(), 1);
    assert_eq!(block[0].kind, NodeKind::Text("hello world"));
    return;
  }

  #[test]
  fn paragraph_break_produces_paragraph_break_node() {
    let block = parse_and_lower("first\n\nsecond");
    assert_eq!(block.len(), 3);
    assert_eq!(block[0].kind, NodeKind::Text("first"));
    assert_eq!(block[1].kind, NodeKind::ParagraphBreak);
    assert_eq!(block[2].kind, NodeKind::Text("second"));
    return;
  }

  #[test]
  fn line_break_produces_line_break_node() {
    let block = parse_and_lower("hello\\\\world");
    assert_eq!(block.len(), 3);
    assert_eq!(block[1].kind, NodeKind::LineBreak);
    return;
  }

  #[test]
  fn escaped_char_becomes_text() {
    let block = parse_and_lower("\\{");
    assert_eq!(block.len(), 1);
    assert_eq!(block[0].kind, NodeKind::Text("{"));
    return;
  }

  #[test]
  fn command_without_args() {
    let block = parse_and_lower("\\foo");
    assert_eq!(block.len(), 1);
    if let NodeKind::Command(cmd) = &block[0].kind {
      assert_eq!(cmd.name, "foo");
      assert!(cmd.args.is_empty());
      assert!(cmd.opt_args.is_empty());
    } else {
      panic!("Command が期待されます");
    }
    return;
  }

  #[test]
  fn command_with_args() {
    let block = parse_and_lower("\\bold{hello}");
    assert_eq!(block.len(), 1);
    if let NodeKind::Command(cmd) = &block[0].kind {
      assert_eq!(cmd.name, "bold");
      assert_eq!(cmd.args.len(), 1);
      assert_eq!(cmd.args[0].len(), 1);
      assert_eq!(cmd.args[0][0].kind, NodeKind::Text("hello"));
    } else {
      panic!("Command が期待されます");
    }
    return;
  }

  #[test]
  fn command_with_optional_and_required_args() {
    let block = parse_and_lower("\\cmd[opt]{arg}");
    assert_eq!(block.len(), 1);
    if let NodeKind::Command(cmd) = &block[0].kind {
      assert_eq!(cmd.name, "cmd");
      assert_eq!(cmd.opt_args.len(), 1);
      assert_eq!(cmd.args.len(), 1);
    } else {
      panic!("Command が期待されます");
    }
    return;
  }

  #[test]
  fn environment_basic() {
    let block = parse_and_lower("\\begin{itemize}\\item{x}\\end{itemize}");
    assert_eq!(block.len(), 1);
    if let NodeKind::Environment(env) = &block[0].kind {
      assert_eq!(env.name, "itemize");
      assert!(!env.children.is_empty());
    } else {
      panic!("Environment が期待されます");
    }
    return;
  }

  #[test]
  fn inline_math() {
    let block = parse_and_lower("$x + y$");
    assert_eq!(block.len(), 1);
    if let NodeKind::InlineMath(math) = &block[0].kind {
      assert!(!math.is_empty());
    } else {
      panic!("InlineMath が期待されます");
    }
    return;
  }

  #[test]
  fn comments_are_removed_in_ast() {
    let block = parse_and_lower("// comment\nhello");
    // コメントは除去され、テキストのみ残る
    let text_nodes: Vec<_> = block.iter().filter(|n| matches!(&n.kind, NodeKind::Text(_))).collect();
    assert!(!text_nodes.is_empty());
    // コメントノードは存在しない
    return;
  }

  #[test]
  fn group_contents_are_flattened() {
    let block = parse_and_lower("{hello}");
    assert_eq!(block.len(), 1);
    assert_eq!(block[0].kind, NodeKind::Text("hello"));
    return;
  }
}
