//! `\caption` コマンドの共通抽出処理

use crate::{
  document::{HirBuilder, HirInline},
  frontend::{
    evaluator::{
      EvalError,
      inline::{IndexPolicy, extract_inline_nodes},
      opt_args::collect_command_opt_args,
    },
    span_ext::ToSourceSpan,
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
pub(super) fn extract_caption(view: &CommandView<'_>, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "caption".to_string(),
      expected: "キャプション本文".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "caption".to_string(),
      span: view.span().to_source_span(),
    });
  }
  return extract_inline_nodes(view.source(), builder, first_arg, IndexPolicy::Allow);
}
