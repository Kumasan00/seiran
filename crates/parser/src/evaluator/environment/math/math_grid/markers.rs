//! 数式グリッド行末マーカー（`\notag` / `\label{...}`）の検出
//!
//! 行末に付く 2 種のマーカーを検出し、走査ローカル状態（`current_notag` / `current_label`）へ
//! 取り込む。マーカー自体はセルに残らないため、後段の数式評価には現れない。

use miette::SourceSpan;
use syntax::{
  SyntaxKind,
  ast::{CommandView, extract_text_content},
  green::GreenElement,
};

use crate::evaluator::EvalError;

/// 行末マーカー `\label{...}` の蓄積を行の `(label, label_span)` に取り出すヘルパ
///
/// `current_label` を消費し、ラベル文字列とその位置をそれぞれの `Option` に分解して返す。
pub(super) fn take_row_label(current_label: &mut Option<(String, SourceSpan)>) -> (Option<String>, Option<SourceSpan>) {
  return match current_label.take() {
    Some((label, span)) => (Some(label), Some(span)),
    None => (None, None),
  };
}

/// 立っている行末マーカーの後ろに意味のある要素が続いていないか検証する
///
/// 列区切り `&` や非トリビア要素がマーカーの後に現れたら、そのマーカーは行末になく不正。
/// `\notag` を先に、続いて `\label` を検査して [`EvalError::NotagNotAtRowEnd`] /
/// [`EvalError::RowLabelNotAtRowEnd`] を返す。
pub(super) fn ensure_markers_at_row_end(
  current_notag: Option<&SourceSpan>,
  current_label: Option<&(String, SourceSpan)>,
) -> Result<(), EvalError> {
  if let Some(span) = current_notag {
    return Err(EvalError::NotagNotAtRowEnd { span: *span });
  }
  if let Some((_, span)) = current_label {
    return Err(EvalError::RowLabelNotAtRowEnd { span: *span });
  }
  return Ok(());
}

/// 行末マーカー `\notag` / `\label{...}` を検出し、走査ローカル状態へ取り込む
///
/// `child` が `\notag` / `\label` の `CommandCall` ノードならそれぞれの検証を行って
/// `current_notag` / `current_label` を立て、`Ok(true)`（呼び出し側は `continue`）を返す。
/// それ以外の要素は `Ok(false)` を返し、呼び出し側はセル要素として扱う。
pub(super) fn try_take_row_marker(
  child: &GreenElement,
  source: &str,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
  current_label: &mut Option<(String, SourceSpan)>,
) -> Result<bool, EvalError> {
  let GreenElement::Node(node) = *child else {
    return Ok(false);
  };
  if node.kind != SyntaxKind::CommandCall {
    return Ok(false);
  }
  let view = CommandView::new(node, source);
  let span: SourceSpan = node.span.into();
  match view.name() {
    "notag" => {
      take_notag_marker(&view, span, row_markers_allowed, current_notag)?;
      return Ok(true);
    },
    "label" => {
      take_label_marker(&view, source, span, row_markers_allowed, current_label)?;
      return Ok(true);
    },
    _ => return Ok(false),
  }
}

/// 行末マーカー `\notag` を検証して走査ローカル状態 `current_notag` を立てる
///
/// `row_markers_allowed` が `false` の環境では [`EvalError::NotagNotSupported`]。`\notag` は引数を
/// 取らないため引数付き・任意引数付き、または 1 行に複数現れた場合は [`EvalError::NotagNotAtRowEnd`]。
fn take_notag_marker(
  view: &CommandView,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_notag: &mut Option<SourceSpan>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
    return Err(EvalError::NotagNotSupported { span });
  }
  // `\notag` は引数を取らない（`\notag{...}` が後続の中身を飲み込むのを防ぐ）
  if !view.args_is_empty() || view.opt_args_count() > 0 {
    return Err(EvalError::NotagNotAtRowEnd { span });
  }
  // 1 行に `\notag` は 1 つだけ
  if current_notag.is_some() {
    return Err(EvalError::NotagNotAtRowEnd { span });
  }
  *current_notag = Some(span);
  return Ok(());
}

/// 行末マーカー `\label{...}` を検証してラベル文字列を抽出し、走査ローカル状態 `current_label` を立てる
///
/// `row_markers_allowed` が `false` の環境では [`EvalError::RowLabelNotSupported`]。`\label` は必須引数
/// 1 個（ラベル名）のみで、引数過不足・任意引数付き、または 1 行に複数現れた場合は
/// [`EvalError::RowLabelNotAtRowEnd`]。
fn take_label_marker(
  view: &CommandView,
  source: &str,
  span: SourceSpan,
  row_markers_allowed: bool,
  current_label: &mut Option<(String, SourceSpan)>,
) -> Result<(), EvalError> {
  if !row_markers_allowed {
    return Err(EvalError::RowLabelNotSupported { span });
  }
  // `\label` は必須引数 1 個（ラベル名）のみ。引数過不足・任意引数付きは不正
  if view.args_count() != 1 || view.opt_args_count() > 0 {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  // 1 行に `\label` は 1 つだけ
  if current_label.is_some() {
    return Err(EvalError::RowLabelNotAtRowEnd { span });
  }
  let first_arg = view.first_arg().ok_or(EvalError::RowLabelNotAtRowEnd { span })?;
  let label = extract_text_content(source, first_arg).trim().to_string();
  *current_label = Some((label, span));
  return Ok(());
}
