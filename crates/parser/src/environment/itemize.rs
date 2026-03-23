//! `itemize` 環境 — 箇条書きリスト
//!
//! `\begin{itemize}...\end{itemize}` 環境内の `\item{...}` コマンドを
//! `DocNode::List` に変換します。

use crate::{
  ast::{CommandView, EnvironmentView},
  document::{DocNode, ListItem},
  evaluator::{EvalError, Evaluator},
  green::GreenElement,
  syntax::SyntaxKind,
};

/// `itemize` 環境を評価する
///
/// # Arguments
///
/// * `view` - 環境の型付きビュー
/// * `evaluator` - 評価器への可変参照
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn itemize(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  if !view.opt_args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "itemize".to_string(),
      span: view.span().into(),
    });
  }

  let mut items = Vec::new();
  let source = view.source();

  if let Some(body) = view.body() {
    for child in body.children {
      if let GreenElement::Node(child_node) = child
        && child_node.kind == SyntaxKind::CommandCall
      {
        let cmd_view = CommandView::new(child_node, source);
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
      }
      // テキストや改行等はスキップ（\item 以外はリスト内では無視）
    }
  }

  return Ok(vec![DocNode::List {
    ordered: false,
    items,
  }]);
}
