//! 本文のページ分割オーケストレーション（段順序自体は `crate::typeset::layout_body` に閉じている）

use super::{error::CompileError, footnote_numbering, image_resources::ImageResources, phase_context::CompileContext};
use crate::typeset::{BodyLayout, BodyLayoutError, BodyLayoutInput};

/// 本文を組版し、確定ページ列と見出し記録を返す。
///
/// ページ単位の脚注採番では、不動点まで本文パスを反復する。
///
/// # Errors
///
/// 画像解決、脚注採番のいずれかに失敗した場合にエラーを返す（ラベル・`\ref` 解決は
/// `resolve::resolve_project` が呼び出し元で既に完了しているため、ここでは失敗しない）。
pub(super) fn typeset_body(
  ctx: &CompileContext<'_>,
  document: &crate::resolve::ResolvedDocument,
  image_resources: &ImageResources,
) -> miette::Result<BodyLayout> {
  let run_pass = |footnote_numbers: Option<&[u32]>| {
    return run_body_pass(ctx, document, image_resources, footnote_numbers);
  };
  return match ctx.style.footnote.numbering {
    crate::config::FootnoteNumbering::Continuous => run_pass(None),
    crate::config::FootnoteNumbering::PerPage => footnote_numbering::solve_per_page_numbering(&run_pass),
  };
}

/// 本文パスを 1 回通す（`crate::typeset::layout_body` の呼び出し + エラーの `miette` 変換）。
///
/// `footnote_numbers` は出現順で引く脚注番号の上書き列。
fn run_body_pass(
  ctx: &CompileContext<'_>,
  document: &crate::resolve::ResolvedDocument,
  image_resources: &ImageResources,
  footnote_numbers: Option<&[u32]>,
) -> miette::Result<BodyLayout> {
  let input = BodyLayoutInput {
    config: ctx.config,
    style: ctx.style,
    resources: ctx.resources,
    text_width: ctx.text_width,
    geometry: &ctx.body_geometry,
    breaker: &crate::typeset::KnuthPlassBreaker,
  };
  // 本文画像は段幅に合わせて解決する
  #[allow(clippy::result_large_err)]
  let resolve_images = |blocks| return resolve_body_images(blocks, ctx, image_resources);
  return match crate::typeset::layout_body(&input, document, footnote_numbers, resolve_images) {
    Ok(layout) => Ok(layout),
    Err(BodyLayoutError::Images { source }) => Err(source.into()),
  };
}

/// 本文画像を段幅に合わせて解決する（`resolve_images` エラー型が大きいため専用関数に切り出す）。
#[allow(clippy::result_large_err)]
fn resolve_body_images(
  blocks: Vec<crate::typeset::Block>,
  ctx: &CompileContext<'_>,
  image_resources: &ImageResources,
) -> Result<Vec<crate::typeset::Block>, CompileError> {
  return super::image_resources::resolve_images(blocks, ctx.body_col_width.to_pt(), image_resources);
}
