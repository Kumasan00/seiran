//! 数式グリッド行末マーカー（`\notag` / `\label{...}`）の検出

use miette::SourceSpan;

use crate::{
  document::{HirBuilder, NodeId},
  frontend::{
    evaluator::EvalError,
    span_ext::ToSourceSpan,
    syntax::{
      SyntaxKind,
      green::GreenElement,
      view::{CommandView, extract_text_content},
    },
  },
};

/// 行末マーカー `\label{...}` から取り込んだ行ラベル
#[derive(Debug)]
pub(crate) struct RowLabel {
  /// 著者が書いたラベル名
  pub(crate) name: String,
  /// 診断に使うマーカーのソース位置
  pub(crate) span: SourceSpan,
  /// マーカー自身の HIR ノード ID（`HirMathRow::label_site` になる）
  pub(crate) site: NodeId,
}

/// 立っている行末マーカーの後ろに意味のある要素が続いていないか検証する
pub(super) fn ensure_markers_at_row_end(
  current_notag: Option<&SourceSpan>,
  current_label: Option<&RowLabel>,
) -> Result<(), EvalError> {
  if let Some(span) = current_notag {
    return Err(EvalError::NotagNotAtRowEnd { span: *span });
  }
  if let Some(label) = current_label {
    return Err(EvalError::RowLabelNotAtRowEnd { span: label.span });
  }
  return Ok(());
}

/// 行末マーカー `\notag` / `\label{...}` を検出し、走査ローカル状態へ取り込む
pub(super) fn try_take_row_marker(
  child: &GreenElement<'_>,
  source: &str,
  builder: &HirBuilder,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
  current_label: &mut Option<RowLabel>,
) -> Result<bool, EvalError> {
  let GreenElement::Node(node) = *child else {
    return Ok(false);
  };
  if node.kind != SyntaxKind::CommandCall {
    return Ok(false);
  }
  let view = CommandView::new(node, source);
  let span: SourceSpan = node.span.to_source_span();
  match view.name() {
    "notag" => {
      take_notag_marker(&view, span, row_markers_allowed, current_notag)?;
      return Ok(true);
    },
    "label" => {
      take_label_marker(&view, source, builder, span, row_markers_allowed, current_label)?;
      return Ok(true);
    },
    _ => return Ok(false),
  }
}

/// 行末マーカー `\notag` を検証して走査ローカル状態 `current_notag` を立てる
fn take_notag_marker(
  view: &CommandView<'_>,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
    return Err(EvalError::NotagNotSupported { span });
  }
  if !view.args_is_empty() || view.opt_args_count() > 0 {
    return Err(EvalError::NotagNotAtRowEnd { span });
  }
  if current_notag.is_some() {
    return Err(EvalError::NotagNotAtRowEnd { span });
  }
  *current_notag = Some(span);
  return Ok(());
}

/// 行末マーカー `\label{...}` を検証してラベル文字列を抽出し、走査ローカル状態 `current_label` を立てる
fn take_label_marker(
  view: &CommandView<'_>,
  source: &str,
  builder: &HirBuilder,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_label: &mut Option<RowLabel>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
    return Err(EvalError::RowLabelNotSupported { span });
  }
  if view.args_count() != 1 || view.opt_args_count() > 0 {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  if current_label.is_some() {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  let first_arg = view.first_arg().ok_or(EvalError::RowLabelNotAtRowEnd { span })?;
  let name = extract_text_content(source, first_arg).trim().to_string();
  *current_label = Some(RowLabel {
    name,
    span,
    site: builder.alloc(view.span()),
  });
  return Ok(());
}
