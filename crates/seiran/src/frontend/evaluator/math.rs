//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`MathNode`] 列に変換する。

use model::{MathNode, MathStyle};

use crate::frontend::{
  evaluator::{EvalError, inline::resolve_symbol_command, opt_args::collect_command_opt_args},
  span_ext::ToSourceSpan,
  syntax::{
    SyntaxKind,
    ast::{CommandView, EnvironmentView},
    green::{GreenElement, GreenNode},
    token::TokenKind,
  },
};

/// インライン数式ノード（`$...$` 由来の `InlineMath`）を `MathNode` のリストに変換する
///
/// # Errors
///
/// 数式内のスタイルコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(crate) fn evaluate_inline_math(source: &str, math_node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  return evaluate_math_children(source, math_node);
}

/// 数式モードで構造化された CST ノードの子要素を `MathNode` 列に変換する共通ヘルパ
fn evaluate_math_children(source: &str, node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  return evaluate_math_elements(source, node.children);
}

/// 数式モードで構造化された要素列を `MathNode` 列に変換する共通ヘルパ
pub(crate) fn evaluate_math_elements(source: &str, elements: &[GreenElement]) -> Result<Vec<MathNode>, EvalError> {
  let mut nodes = Vec::new();
  for child in elements {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::Ampersand => {
          return Err(EvalError::UnsupportedInMath {
            what: r"&（列区切り）".to_string(),
            span: token.span.to_source_span(),
          });
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（行区切り）".to_string(),
            span: token.span.to_source_span(),
          });
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, child_node)?;
          nodes.push(math_node);
        },
        SyntaxKind::MathGroup => {
          let inner = evaluate_math_children(source, child_node)?;
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::MathSubscript => {
          let inner = evaluate_math_script_content(source, child_node)?;
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Subscript(Box::new(content)));
        },
        SyntaxKind::MathSuperscript => {
          let inner = evaluate_math_script_content(source, child_node)?;
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Superscript(Box::new(content)));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::UnsupportedInMath {
            what: format!("環境 {}", view.name()),
            span: child_node.span.to_source_span(),
          });
        },
        _ => {},
      },
    }
  }
  return Ok(nodes);
}

/// 上付き・下付きスクリプトノードの中身を `MathNode` に変換する
fn evaluate_math_script_content(source: &str, script_node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  let mut nodes = Vec::new();
  for child in script_node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Comma | TokenKind::Equals => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（強制改行）".to_string(),
            span: token.span.to_source_span(),
          });
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::MathGroup => {
          let inner = evaluate_math_children(source, child_node)?;
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, child_node)?;
          nodes.push(math_node);
        },
        _ => {},
      },
    }
  }
  return Ok(nodes);
}

/// 数式内コマンドを `MathNode` に変換する
fn evaluate_math_command(source: &str, cmd_node: &GreenNode) -> Result<MathNode, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let name = view.name();

  // 数式スタイルコマンド（\mathbold, \mathitalic 等）
  if let Some(style) = MathStyle::from_command_name(name) {
    let _opt_args = collect_command_opt_args(&view, &[])?;
    let arg_count = view.args().count();
    if arg_count == 0 {
      return Err(EvalError::MissingCommandArgument {
        name: name.to_string(),
        expected: "1 個（数式本体）".to_string(),
        span: view.span().to_source_span(),
      });
    }
    if arg_count > 1 {
      return Err(EvalError::ExtraCommandArgument {
        name: name.to_string(),
        span: view.span().to_source_span(),
      });
    }
    #[allow(clippy::unwrap_used)]
    let first_arg = view.first_arg().unwrap();
    let body = evaluate_inline_math(source, first_arg)?;
    return Ok(MathNode::Styled { style, body });
  }

  match name {
    "frac" => {
      let _opt_args = collect_command_opt_args(&view, &[])?;
      if view.args_count() > 2 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      let mut args = view.args();
      let (Some(numer_arg), Some(denom_arg)) = (args.next(), args.next()) else {
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "2 個（分子と分母）".to_string(),
          span: view.span().to_source_span(),
        });
      };
      return Ok(MathNode::Frac {
        numer: Box::new(math_arg_to_node(source, numer_arg)?),
        denom: Box::new(math_arg_to_node(source, denom_arg)?),
      });
    },
    "sqrt" => {
      if view.opt_args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      if view.args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      let index = match view.opt_args().next() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, opt)?)),
        None => None,
      };
      let Some(radicand_arg) = view.first_arg() else {
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "1 個（被開平数）".to_string(),
          span: view.span().to_source_span(),
        });
      };
      return Ok(MathNode::Sqrt {
        index,
        radicand: Box::new(math_arg_to_node(source, radicand_arg)?),
      });
    },
    _ => {
      if let Some(ch) = resolve_symbol_command(name) {
        let _opt_args = collect_command_opt_args(&view, &[])?;
        if !view.args_is_empty() {
          return Err(EvalError::ExtraCommandArgument {
            name: name.to_string(),
            span: view.span().to_source_span(),
          });
        }
        return Ok(MathNode::Symbol(ch));
      }

      return Err(EvalError::UnknownCommand {
        name: name.to_string(),
        span: view.span().to_source_span(),
      });
    },
  }
}

/// 数式引数ノードを単一の `MathNode` に変換するヘルパー
fn math_arg_to_node(source: &str, arg_node: &GreenNode) -> Result<MathNode, EvalError> {
  let nodes = evaluate_inline_math(source, arg_node)?;
  if nodes.len() == 1 {
    #[allow(clippy::unwrap_used)]
    return Ok(nodes.into_iter().next().unwrap());
  }
  return Ok(MathNode::Group(nodes));
}
