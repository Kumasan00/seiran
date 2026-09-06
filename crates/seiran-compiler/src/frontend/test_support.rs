//! frontend 配下と後段（semantics / typeset）の test module が共有する、resolver 注入済みの入口。
//!
//! 本番の `parse_source` は `compile` facade が構築した `PathResolver` を要求する。テストは画像パスの
//! 解決を見ないものが大半なので、**空の `base_dir`**（相対パスがそのまま残る）を注入した形を 1 箇所に置く。
//! パス解決そのものを検証するテストは `PathResolver::new(Path::new("/project"))` を明示して
//! `frontend::parse_source` を直接呼ぶ。

use std::path::Path;

use crate::{
  document::{HirBuilder, HirSource},
  frontend::{self, ParseSourceError},
  project::PathResolver,
  source::SourceId,
};

/// 相対パスをそのまま残す resolver（`base_dir` が空パス）。
///
/// `\image{a.png}` は `ProjectPath::new("a.png")` として HIR に載る。
fn unbased_resolver() -> PathResolver { return PathResolver::new(Path::new("")); }

/// [`frontend::parse_source`] を空の `base_dir` で呼ぶ。
///
/// # Errors
///
/// 構文エラーまたは評価エラーをそのまま返す。
pub(crate) fn parse_source_for_test(source: &str, source_id: SourceId) -> Result<HirSource, ParseSourceError> {
  return frontend::parse_source(source, source_id, &unbased_resolver());
}

/// `SourceId(0)` と空の `base_dir` の builder。ハンドラを直接呼ぶテスト用。
pub(crate) fn hir_builder_for_test() -> HirBuilder { return HirBuilder::new(SourceId::new(0), unbased_resolver()); }
