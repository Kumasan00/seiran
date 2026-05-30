//! リスト環境 — 箇条書き・番号付きリスト
//!
//! `\begin{itemize}...\end{itemize}` および `\begin{enumerate}...\end{enumerate}`
//! 環境内の `\item{...}` コマンドを `DocNode::List` に変換します。

use syntax::ast::EnvironmentView;

use crate::{
  document::{DocNode, ListItem},
  evaluator::{EvalError, Evaluator, environment::body_scan, opt_args::collect_environment_opt_args},
};

/// リスト環境の共通処理
///
/// `\item{...}` コマンドを収集し、`DocNode::List` を生成します。
///
/// # Arguments
///
/// * `view` - 環境の型付きビュー
/// * `evaluator` - 評価器への可変参照
/// * `ordered` - 順序付きリストかどうか
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
fn list_common(view: &EnvironmentView, evaluator: &mut Evaluator, ordered: bool) -> Result<Vec<DocNode>, EvalError> {
  let _opt_args = collect_environment_opt_args(view, &[])?;

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
  return list_common(view, evaluator, false);
}

/// `enumerate` 環境を評価する（順序付きリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn enumerate(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  return list_common(view, evaluator, true);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn itemize_rejects_unknown_opt_arg_key() {
    // Arrange — itemize は任意引数を受け付けないので `[noitemsep]` は不明キー
    let arena = Bump::new();
    let source = r"\begin{itemize}[noitemsep]\item{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "noitemsep"));
  }
}
