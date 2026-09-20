//! `\caption` コマンドの共通抽出処理

use crate::{
  document::HirInline,
  frontend::{
    evaluator::{
      EvalContext, EvalError, arity,
      inline::{IndexPolicy, extract_inline_nodes},
      opt_args,
    },
    syntax::view::CommandView,
  },
};

/// `\caption{...}` の引数をインライン要素列に変換する
///
/// キャプションは図表 1 個につき 1 箇所にしか置かれないので `\index` を許可する。
///
/// # Errors
///
/// 引数の不足・過剰、未許可の任意引数がある場合にエラーを返します。
pub(super) fn extract_caption(view: &CommandView<'_>, ctx: &EvalContext<'_>) -> Result<Vec<HirInline>, EvalError> {
  opt_args::no_command_opt_args(view)?;
  let first_arg = arity::exactly_one_arg(view, "キャプション本文")?;
  return extract_inline_nodes(view.source(), ctx, first_arg, IndexPolicy::Allow);
}
