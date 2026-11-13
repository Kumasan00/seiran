//! `\caption` コマンドの共通評価処理

use crate::{
  document::HirInline,
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{IndexPolicy, evaluate_inline_children},
      signature::Signature,
    },
    syntax::{ArgMode, CstNode, view::CommandView},
  },
};

/// `\caption` の署名（図と表が共有する）
pub(super) const SIGNATURE: Signature<1> = Signature::new([ArgMode::Inherit], "キャプション本文", &[]);

/// `\caption{...}` の引数をインライン要素列に変換する
///
/// キャプションは図表 1 個につき 1 箇所にしか置かれないので `\index` を許可する。
///
/// # Errors
///
/// 本文の評価エラーを返します。
pub(super) fn evaluate_caption(
  view: &CommandView<'_>,
  arg: &CstNode<'_>,
  ctx: &EvalContext<'_>,
) -> Result<Vec<HirInline>, EvalError> {
  return evaluate_inline_children(view.source(), ctx, arg, IndexPolicy::Allow);
}
