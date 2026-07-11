//! リスト環境 — 箇条書き・番号付きリスト
//!
//! `\begin{itemize}...\end{itemize}` および `\begin{enumerate}...\end{enumerate}`
//! 環境内の `\item{...}` コマンドを `DocNode::List` に変換します。

use document::{DocNode, ListItem};
use syntax::ast::EnvironmentView;

use crate::evaluator::{
  EvalError,
  environment::body_scan,
  opt_args::{collect_command_opt_args, collect_environment_opt_args},
};

/// `itemize` 環境を評価する（順序なしリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn itemize(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> { return list_common(view, false); }

/// `enumerate` 環境を評価する（順序付きリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn enumerate(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> { return list_common(view, true); }

/// リスト環境の共通処理
///
/// `\item{...}` コマンドを収集し、`DocNode::List` を生成します。
/// body 直下の `\item` 以外のコンテンツ（テキスト・他コマンド等）は黙って捨てず
/// エラーとして報告します。
///
/// # Arguments
///
/// * `view` - 環境の型付きビュー
/// * `ordered` - 順序付きリストかどうか
///
/// # Errors
///
/// 余分な引数、body 直下の許可外コンテンツ、`\item` の引数不足・過剰の場合にエラーを返します
fn list_common(view: &EnvironmentView, ordered: bool) -> Result<Vec<DocNode>, EvalError> {
  let _opt_args = collect_environment_opt_args(view, &[])?;
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }

  let mut items = Vec::new();
  let source = view.source();

  if let Some(body) = view.body() {
    for cmd_view in body_scan::strict_command_calls(source, body, view.name(), &["item"], "\\item{...}")? {
      let _item_opt_args = collect_command_opt_args(&cmd_view, &[])?;
      let Some(first_arg) = cmd_view.first_arg() else {
        return Err(EvalError::MissingCommandArgument {
          name: "item".to_string(),
          expected: "項目の内容".to_string(),
          span: cmd_view.span().into(),
        });
      };
      if cmd_view.args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: "item".to_string(),
          span: cmd_view.span().into(),
        });
      }
      let content = crate::evaluator::evaluate_children(source, first_arg)?;
      items.push(ListItem { content });
    }
  }

  return Ok(vec![DocNode::List { ordered, items }]);
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

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "noitemsep"));
  }
}
