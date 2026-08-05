//! `\row` の `&` 区切り区画からセル（[`HirTableCell`]）を構築する

use crate::{
  frontend::{
    evaluator::{
      EvalError,
      inline::{extract_inline_nodes, extract_inline_nodes_from_elements},
      opt_args::{OptType, OptValue, collect_command_opt_args},
    },
    span_ext::ToSourceSpan,
    syntax::{SyntaxKind, ast::CommandView, green::GreenElement, token::TokenKind},
  },
  model::{HirBuilder, HirInline, HirInlineKind, HirTableCell, Span},
};

/// `&` 分割後の 1 区画を [`HirTableCell`] に変換する
pub(super) fn build_cell(
  source: &str,
  builder: &HirBuilder,
  elements: &[GreenElement],
  empty_span: Span,
) -> Result<HirTableCell, EvalError> {
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
                span: node.span.to_source_span(),
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
        span: cell_cmd.span().to_source_span(),
      });
    }
    return extract_cell_command(&cell_cmd, builder);
  }

  let id = builder.alloc(segment_span(elements, empty_span));
  let content = extract_inline_nodes_from_elements(source, builder, elements)?;
  return Ok(HirTableCell {
    id,
    content: trim_cell_content(content),
    span: 1,
  });
}

/// `\cell[span=N]{...}` を属性付きセルに変換する
fn extract_cell_command(view: &CommandView, builder: &HirBuilder) -> Result<HirTableCell, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("span", OptType::Number)])?;
  let mut span: u32 = 1;
  for (key, value) in opt_args {
    if let ("span", OptValue::Number(n)) = (key.as_str(), value) {
      if !(n.is_finite() && n >= 1.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)) {
        return Err(EvalError::InvalidOptArgValue {
          name: "cell".to_string(),
          key: "span".to_string(),
          expected: "1 以上の整数".to_string(),
          span: view.span().to_source_span(),
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
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "cell".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let id = builder.alloc(view.span());
  let content = trim_cell_content(extract_inline_nodes(view.source(), builder, arg)?);
  return Ok(HirTableCell { id, content, span });
}

/// セル内容の前後の空白由来 `Text` ノードをトリムする
///
/// 境界にある空白だけのノードと、境界のテキスト端を削る。
fn trim_cell_content(mut content: Vec<HirInline>) -> Vec<HirInline> {
  while matches!(content.first().map(|inline| return &inline.kind), Some(HirInlineKind::Text(t)) if t.trim().is_empty())
  {
    content.remove(0);
  }
  if let Some(HirInlineKind::Text(t)) = content.first_mut().map(|inline| return &mut inline.kind) {
    *t = t.trim_start().to_string();
  }
  while matches!(content.last().map(|inline| return &inline.kind), Some(HirInlineKind::Text(t)) if t.trim().is_empty())
  {
    content.pop();
  }
  if let Some(HirInlineKind::Text(t)) = content.last_mut().map(|inline| return &mut inline.kind) {
    *t = t.trim_end().to_string();
  }
  return content;
}

/// `&` 分割後の区画全体を覆うソース位置を返す
///
/// 区画が空（`a & & b` の中央など）なら、呼び出し元が渡した直前の区切り位置を使う。
fn segment_span(elements: &[GreenElement], empty_span: Span) -> Span {
  let mut span: Option<Span> = None;
  for element in elements {
    let element_span = match element {
      GreenElement::Token(token) => token.span,
      GreenElement::Node(node) => node.span,
    };
    span = Some(span.map_or(element_span, |current| return current.merge(element_span)));
  }
  return span.unwrap_or(empty_span);
}

/// セル内容に強制改行（`\\`）が含まれるかを再帰的に判定する
pub(super) fn contains_line_break(nodes: &[HirInline]) -> bool {
  return nodes.iter().any(|node| match &node.kind {
    HirInlineKind::LineBreak => return true,
    HirInlineKind::Styled { children, .. } | HirInlineKind::Colored { children, .. } => {
      return contains_line_break(children);
    },
    _ => return false,
  });
}
