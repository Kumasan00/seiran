//! 環境本体（`EnvironmentBody`）を走査する共通ヘルパ
//!
//! 図環境（`figure`）・リスト環境（`itemize`/`enumerate`）・表環境（`tabular`）など
//! 複数の環境ハンドラから利用される **body 直下の `CommandCall` 走査** を一本化する。
//!
//! ## 設計
//!
//! - body の子要素のうち [`SyntaxKind::CommandCall`] のみを返すイテレータを提供
//! - テキスト・空白・改行・コメントなどは黙ってスキップ
//! - 各ハンドラはコマンド名による `match` で `\item` / `\caption` / `\label` を捌く
//!
//! ## 使用例
//!
//! ```ignore
//! for view in body_scan::iter_command_calls(source, body) {
//!   match view.name() {
//!     "item" => { /* ... */ }
//!     _ => {}
//!   }
//! }
//! ```

use syntax::{
  SyntaxKind,
  ast::CommandView,
  green::{GreenElement, GreenNode},
};

/// 環境本体の直下にある `CommandCall` ノードを順に返すイテレータ
///
/// `body.children` を走査し、`SyntaxKind::CommandCall` のノードを `CommandView` で
/// ラップして yield する。それ以外（テキストトークン・トリビア・別種別ノード）は
/// スキップする。
#[allow(dead_code)]
pub(crate) fn iter_command_calls<'a>(
  source: &'a str,
  body: &'a GreenNode<'a>,
) -> impl Iterator<Item = CommandView<'a>> + 'a {
  return body.children.iter().filter_map(move |child| {
    if let GreenElement::Node(node) = child
      && node.kind == SyntaxKind::CommandCall
    {
      return Some(CommandView::new(node, source));
    }
    return None;
  });
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;
  use syntax::parse;

  use super::*;

  #[test]
  fn iter_command_calls_yields_only_command_calls() {
    // \begin{itemize} の body には \item の CommandCall とテキストが混在するが、
    // iter_command_calls は CommandCall だけを返す
    let arena = Bump::new();
    let source = r"\begin{itemize}\item{A}some text\item{B}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    let env = cst.children.iter().find_map(|c| match c {
      syntax::green::GreenElement::Node(n) if n.kind == SyntaxKind::Environment => Some(n),
      _ => None,
    });
    let env = env.expect("Environment ノードが期待されます");
    let body = env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();

    let names: Vec<&str> = iter_command_calls(source, body).map(|v| v.name()).collect();
    assert_eq!(names, vec!["item", "item"]);
  }
}
