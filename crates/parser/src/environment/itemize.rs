//! リスト環境 — 箇条書き・番号付きリスト
//!
//! `\begin{itemize}...\end{itemize}` および `\begin{enumerate}...\end{enumerate}`
//! 環境内の `\item{...}` コマンドを `DocNode::List` に変換します。

use crate::{
  ast::EnvironmentView,
  document::{DocNode, ListItem},
  environment::body_scan,
  evaluator::{EvalError, Evaluator},
};

/// リスト環境の共通処理
///
/// `\item{...}` コマンドを収集し、`DocNode::List` を生成します。
///
/// # Arguments
///
/// * `view` - 環境の型付きビュー
/// * `evaluator` - 評価器への可変参照
/// * `env_name` - 環境名（エラーメッセージ用）
/// * `ordered` - 順序付きリストかどうか
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
fn list_common(
  view: &EnvironmentView,
  evaluator: &mut Evaluator,
  env_name: &str,
  ordered: bool,
) -> Result<Vec<DocNode>, EvalError> {
  if !view.opt_args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: env_name.to_string(),
      span: view.span().into(),
    });
  }

  let mut items = Vec::new();
  let source = view.source();

  if let Some(body) = view.body() {
    for cmd_view in body_scan::iter_command_calls(source, body) {
      if cmd_view.name() == "item" {
        let mut item_content = Vec::new();
        for arg in cmd_view.args() {
          let doc_nodes = evaluator.evaluate_children(source, arg)?;
          item_content.extend(doc_nodes);
        }
        items.push(ListItem {
          content: item_content,
        });
      }
      // \item 以外の CommandCall（万一登場した場合）は無視
    }
  }

  return Ok(vec![DocNode::List { ordered, items }]);
}

/// `itemize` 環境を評価する（順序なしリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn itemize(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return list_common(view, evaluator, "itemize", false);
}

/// `enumerate` 環境を評価する（順序付きリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn enumerate(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return list_common(view, evaluator, "enumerate", true);
}
