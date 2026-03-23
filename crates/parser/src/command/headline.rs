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
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `name` - コマンド名（エラーメッセージ用）
/// * `level` - 見出しレベル
/// * `expected` - 引数の説明（エラーメッセージ用）
/// * `context` - 評価コンテキスト（採番用）
///
/// # Errors
///
/// 引数不足・過剰の場合にエラーを返します
fn heading_common(
  view: &CommandView,
  name: &str,
  level: HeadingLevel,
  expected: &str,
  context: &mut EvalContext,
) -> Result<Vec<DocNode>, EvalError> {
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: expected.to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 || !view.opt_args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  let title = extract_inline_nodes(view.source(), first_arg);
  let number = HeadingNumber::from_context(level, context);

  return Ok(vec![DocNode::Heading {
    level,
    number,
    title,
  }]);
}

/// `\part{タイトル}` — 部見出し
#[inline]
pub(super) fn part(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.part += 1;
  context.chapter = 0;
  context.section = 0;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  return heading_common(view, "part", HeadingLevel::Part, "部名", context);
}

/// `\chapter{タイトル}` — 章見出し
#[inline]
pub(super) fn chapter(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.chapter += 1;
  context.section = 0;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  return heading_common(view, "chapter", HeadingLevel::Chapter, "章名", context);
}

/// `\section{タイトル}` — セクション見出し
#[inline]
pub(super) fn section(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.section += 1;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  return heading_common(view, "section", HeadingLevel::Section, "セクション名", context);
}

/// `\subsection{タイトル}` — サブセクション見出し
#[inline]
pub(super) fn subsection(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.subsection += 1;
  context.paragraph = 0;
  context.subparagraph = 0;
  return heading_common(view, "subsection", HeadingLevel::Subsection, "サブセクション名", context);
}

/// `\paragraph{タイトル}` — 段落見出し
#[inline]
pub(super) fn paragraph(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.paragraph += 1;
  context.subparagraph = 0;
  return heading_common(view, "paragraph", HeadingLevel::Paragraph, "段落名", context);
}

/// `\subparagraph{タイトル}` — 小段落見出し
#[inline]
pub(super) fn subparagraph(view: &CommandView, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.subparagraph += 1;
  return heading_common(view, "subparagraph", HeadingLevel::Subparagraph, "小節名", context);
}
