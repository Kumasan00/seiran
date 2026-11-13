//! リスト環境 — 箇条書き・番号付きリスト

use crate::{
  document::{HirList, HirListItem, HirNode, HirNodeKind},
  frontend::{
    evaluator::{
      self, EvalContext, EvalError,
      environment::body_scan,
      opt_args::{self, OptArgs, OptDecl, OptKey},
      signature::{Bound, Signature},
    },
    syntax::{ArgMode, view::EnvironmentView},
  },
  length::Length,
};

/// `enumerate[start=N]`（開始番号。1 以上の整数）
const START: OptKey<u32> = opt_args::positive_int("start");
/// リスト環境と `\item` の `[item_gap=...]`（項目間のアキ）
const ITEM_GAP: OptKey<Length> = opt_args::length("item_gap");
/// `\item[marker=...]`（マーカーの上書き）
const MARKER: OptKey<String> = opt_args::string("marker");
/// 順序付きリストのスキーマ
const ORDERED_SCHEMA: &[OptDecl] = &[START.decl(), ITEM_GAP.decl()];
/// 順序なしリストのスキーマ
const UNORDERED_SCHEMA: &[OptDecl] = &[ITEM_GAP.decl()];
/// `\item` が受け付けるキー
const ITEM_KEYS: &[OptDecl] = &[MARKER.decl(), ITEM_GAP.decl()];
/// `\item` の署名
const ITEM: Signature<1> = Signature::new([ArgMode::Inherit], "項目の内容", ITEM_KEYS);

/// リスト環境の署名（`start` を受け付けるのは番号付きだけ）
pub(super) fn signature(ordered: bool) -> Signature<0> {
  return if ordered {
    Signature::keys_only(ORDERED_SCHEMA)
  } else {
    Signature::keys_only(UNORDERED_SCHEMA)
  };
}

/// リスト環境（`itemize` / `enumerate`）を評価する
///
/// `ordered` は番号付き（`enumerate`）かどうか。
///
/// # Errors
///
/// body 直下の許可外コンテンツ、`\item` の署名の検査のエラーの場合にエラーを返します
pub(super) fn list(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  opts: &OptArgs,
  ordered: bool,
) -> Result<HirNode, EvalError> {
  let item_gap = opts.get(ITEM_GAP);
  let start = opts.get(START);

  let id = ctx.alloc(view.span());
  let mut items = Vec::new();
  let source = view.source();

  if let Some(body) = view.body() {
    for (
      (),
      cmd_view,
      Bound {
        args: [arg],
        opts: item_opts,
      },
    ) in
      body_scan::collect_allowed_commands(source, body.children, view.name(), &[("item", (), ITEM)], "\\item{...}")?
    {
      let marker = item_opts.get(MARKER);
      let item_gap = item_opts.get(ITEM_GAP);
      let item_id = ctx.alloc(cmd_view.span());
      let item_body = evaluator::evaluate_children(source, ctx, arg)?;
      items.push(HirListItem {
        id: item_id,
        body: item_body,
        marker,
        item_gap,
      });
    }
  }

  return Ok(HirNode::new(
    id,
    HirNodeKind::List(HirList {
      ordered,
      items,
      start,
      item_gap,
    }),
  ));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, test_support};

  #[test]
  fn itemize_rejects_unknown_opt_arg_key() {
    let arena = Bump::new();
    let source = r"\begin{itemize}[noitemsep]\item{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "noitemsep"));
  }

  #[test]
  fn enumerate_start_option_sets_list_start() {
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=5]\item{A}\end{enumerate}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.start, Some(5));
  }

  #[test]
  fn itemize_rejects_start_opt_arg_key() {
    let arena = Bump::new();
    let source = r"\begin{itemize}[start=5]\item{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "start"));
  }

  #[test]
  fn enumerate_start_zero_is_invalid() {
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=0]\item{A}\end{enumerate}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "start"));
  }

  #[test]
  fn item_marker_option_sets_list_item_marker() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[marker=☆]{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.items[0].marker, Some("☆".to_string()));
  }

  #[test]
  fn item_marker_option_accepts_empty_string() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[marker=]{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.items[0].marker, Some(String::new()));
  }

  #[test]
  fn item_rejects_unknown_opt_arg_key_other_than_marker() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[foo=bar]{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn itemize_item_gap_option_sets_list_item_gap() {
    let arena = Bump::new();
    let source = r"\begin{itemize}[item_gap=0pt]\item{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.item_gap, Some(Length::ZERO));
  }

  #[test]
  fn enumerate_item_gap_option_combines_with_start() {
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=2, item_gap=8mm]\item{A}\end{enumerate}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.start, Some(2));
    assert_eq!(list.item_gap, Some(Length::mm(8.0)));
  }

  #[test]
  fn item_gap_option_accepts_negative_value() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[item_gap=-1mm]{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::List(list) = &nodes[0].kind else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(list.items[0].item_gap, Some(Length::mm(-1.0)));
  }
}
