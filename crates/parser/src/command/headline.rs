//! 見出しコマンド群
//!
//! `\part`, `\chapter`, `\section`, `\subsection`, `\paragraph`, `\subparagraph`
//! コマンドの実装です。各コマンドは見出しレベルに応じた `DocNode::Heading` を生成し、
//! 自動採番を行います。

use crate::{
  ast::{CommandView, extract_inline_nodes},
  document::{DocNode, HeadingLevel, HeadingNumber},
  evaluator::{EvalContext, EvalError},
};

/// 見出しコマンドの共通処理
///
/// カウンタのインクリメント・リセット、引数バリデーション、
/// `DocNode::Heading` の生成をすべて行います。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `level` - 見出しレベル
/// * `context` - 評価コンテキスト（採番用）
///
/// # Errors
///
/// 引数不足・過剰の場合にエラーを返します
pub(super) fn heading(
  view: &CommandView,
  level: HeadingLevel,
  context: &mut EvalContext,
) -> Result<Vec<DocNode>, EvalError> {
  let name = level.command_name();

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: level.expected_name().to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 || !view.opt_args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  context.increment_heading(level);
  let title = extract_inline_nodes(view.source(), first_arg);
  let number = HeadingNumber::from_context(level, context);

  return Ok(vec![DocNode::Heading {
    level,
    number,
    title,
    label: None,
  }]);
}
