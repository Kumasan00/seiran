//! `\row` の `&` 区切り区画からセル（[`TableCell`]）を構築する

use document::{InlineNode, TableCell};
use syntax::{SyntaxKind, ast::CommandView, green::GreenElement, token::TokenKind};

use crate::evaluator::{
  EvalError,
  inline::{extract_inline_nodes, extract_inline_nodes_from_elements},
  opt_args::{OptType, OptValue, collect_command_opt_args},
};

/// `&` 分割後の 1 区画を [`TableCell`] に変換する
///
/// 区画の非トリビア要素が `\cell` コマンド 1 つだけなら属性付きセルとして評価し、
/// `\cell` と他の内容が混在していればエラーにする。それ以外の区画は
/// インライン内容として評価し、前後の空白をトリムする。
pub(super) fn build_cell(source: &str, elements: &[GreenElement]) -> Result<TableCell, EvalError> {
  let mut cell_view: Option<CommandView> = None;
  let mut has_other_content = false;
  for element in elements {
    match element {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comment | TokenKind::LBrace | TokenKind::RBrace => {},
        _ => has_other_content = true,
      },
      GreenElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let candidate = CommandView::new(node, source);
          if candidate.name() == "cell" {
            if cell_view.is_some() {
              // 同一区画に \cell が 2 つ — `&` の書き忘れ
              return Err(EvalError::TableCellMixedContent {
                span: node.span.into(),
              });
            }
            cell_view = Some(candidate);
          } else {
            has_other_content = true;
          }
        } else {
          has_other_content = true;
        }
      },
    }
  }

  if let Some(cell_cmd) = cell_view {
    if has_other_content {
      return Err(EvalError::TableCellMixedContent {
        span: cell_cmd.span().into(),
      });
    }
    return extract_cell_command(&cell_cmd);
  }

  let content = extract_inline_nodes_from_elements(source, elements)?;
  return Ok(TableCell::new(trim_cell_content(content)));
}

/// `\cell[span=N]{...}` を属性付きセルに変換する
fn extract_cell_command(view: &CommandView) -> Result<TableCell, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("span", OptType::Number)])?;
  let mut span: u32 = 1;
  for (key, value) in opt_args {
    if let ("span", OptValue::Number(n)) = (key.as_str(), value) {
      // span は 1 以上の整数のみ受理する
      if !(n.is_finite() && n >= 1.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)) {
        return Err(EvalError::InvalidOptArgValue {
          name: "cell".to_string(),
          key: "span".to_string(),
          expected: "1 以上の整数".to_string(),
          span: view.span().into(),
        });
      }
      #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
      {
        span = n as u32;
      }
    }
  }

  let Some(arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "cell".to_string(),
      expected: "セル内容".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "cell".to_string(),
      span: view.span().into(),
    });
  }

  let content = trim_cell_content(extract_inline_nodes(view.source(), arg)?);
  return Ok(TableCell { content, span });
}

/// セル内容の前後の空白由来 `Text` ノードをトリムする
///
/// `\row{Alice & 92}` の区切り前後の空白がセル内容に残ると揃えがずれるため、
/// 先頭・末尾の空白のみの `Text` を除去し、境界の `Text` の端も削る。
fn trim_cell_content(mut content: Vec<InlineNode>) -> Vec<InlineNode> {
  while matches!(content.first(), Some(InlineNode::Text(t)) if t.trim().is_empty()) {
    content.remove(0);
  }
  if let Some(InlineNode::Text(t)) = content.first_mut() {
    *t = t.trim_start().to_string();
  }
  while matches!(content.last(), Some(InlineNode::Text(t)) if t.trim().is_empty()) {
    content.pop();
  }
  if let Some(InlineNode::Text(t)) = content.last_mut() {
    *t = t.trim_end().to_string();
  }
  return content;
}

/// セル内容に強制改行（`\\`）が含まれるかを再帰的に判定する
pub(super) fn contains_line_break(nodes: &[InlineNode]) -> bool {
  return nodes.iter().any(|node| match node {
    InlineNode::LineBreak => true,
    InlineNode::Styled { children, .. } | InlineNode::Colored { children, .. } => contains_line_break(children),
    _ => false,
  });
}
