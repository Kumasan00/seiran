//! frontend 配下と後段（semantics / typeset）の test module が共有する、resolver 注入済みの入口。
//!
//! 本番の `parse_source` は `compile` facade が構築した `PathResolver` を要求する。テストは画像パスの
//! 解決を見ないものが大半なので、**空の `base_dir`**（相対パスがそのまま残る）を注入した形を 1 箇所に置く。
//! パス解決そのものを検証するテストは `PathResolver::new(Path::new("/project"))` を明示して
//! `frontend::parse_source` を直接呼ぶ。

use std::{path::Path, sync::LazyLock};

use crate::{
  document::HirSource,
  frontend::{self, ParseSourceError, evaluator::EvalContext},
  project::PathResolver,
  source::SourceId,
};

/// 相対パスをそのまま残す resolver（`base_dir` が空パス）。
///
/// `\image{a.png}` は `ProjectPath::new("a.png")` として HIR に載る。`EvalContext` が resolver を
/// 借用するので、context を値で返せるよう `'static` に置く。
static UNBASED_RESOLVER: LazyLock<PathResolver> = LazyLock::new(|| return PathResolver::new(Path::new("")));

/// [`frontend::parse_source`] を空の `base_dir` で呼ぶ。
///
/// # Errors
///
/// 構文エラーまたは評価エラーをそのまま返す。
pub(crate) fn parse_source_for_test(source: &str, source_id: SourceId) -> Result<HirSource, ParseSourceError> {
  return frontend::parse_source(source, source_id, &UNBASED_RESOLVER);
}

/// `SourceId(0)` と空の `base_dir` の評価 context。ハンドラを直接呼ぶテスト用。
pub(crate) fn eval_context_for_test() -> EvalContext<'static> {
  return EvalContext::new(SourceId::new(0), &UNBASED_RESOLVER);
}
