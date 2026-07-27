//! 本文のブロック構築とページ分割

use std::time::Instant;

use tracing::{debug_span, info};
use typeset::LoweringContext;

use super::{
  ParsedProject, elapsed_ms, footnote_numbering, image_resources::ImageResources, phase_context::CompileContext,
  wrap_lowering_error,
};

/// 本文パス 1 回ぶんの出力。
#[derive(Debug)]
pub(super) struct BodyLayout {
  /// 確定した本文ページ列
  pub(super) pages: Vec<typeset::Page>,
  /// 目次・しおり用の見出し情報（文書順）
  pub(super) headings: Vec<typeset::HeadingRecord>,
}

/// 本文を組版し、確定ページ列と見出し記録を返す。
///
/// ページ単位の脚注採番では、不動点まで本文パスを反復する。
///
/// # Errors
///
/// lowering、画像解決、脚注採番のいずれかに失敗した場合にエラーを返す。
pub(super) fn typeset_body(
  ctx: &CompileContext<'_>,
  parsed_project: &ParsedProject,
  image_resources: &ImageResources,
) -> miette::Result<BodyLayout> {
  let groups = parsed_project.lowering_groups();
  let run_pass = |footnote_numbers: Option<&[u32]>| {
    return run_body_pass(ctx, parsed_project, &groups, image_resources, footnote_numbers);
  };
  return match ctx.style.footnote.numbering {
    config::FootnoteNumbering::Continuous => run_pass(None),
    config::FootnoteNumbering::PerPage => footnote_numbering::solve_per_page_numbering(&run_pass),
  };
}

/// 本文パスを 1 回通す（lowering → `build_blocks` → 画像サイズ確定 → `break_pages`）。
///
/// `footnote_numbers` は出現順で引く脚注番号の上書き列。
fn run_body_pass(
  ctx: &CompileContext<'_>,
  parsed_project: &ParsedProject,
  groups: &[typeset::SourceGroup<'_>],
  image_resources: &ImageResources,
  footnote_numbers: Option<&[u32]>,
) -> miette::Result<BodyLayout> {
  let stage_start = Instant::now();
  let mut lowering_ctx =
    LoweringContext::new(ctx.style).with_image_defaults(ctx.config.image.max_dpi, ctx.config.image.downsample);
  if let Some(numbers) = footnote_numbers {
    lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
  }
  let (body_layout_nodes, headings) = typeset::lower_sources_with_headings(&lowering_ctx, groups)
    .map_err(|error| return wrap_lowering_error(error, &parsed_project.parsed))?;
  info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    typeset::build_blocks(
      body_layout_nodes,
      ctx.resources,
      ctx.style.text.font_size,
      ctx.style.text.line_height_factor,
      ctx.config.document.language.as_deref(),
      ctx.style.text.punctuation_spacing,
    )
  };
  info!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  let stage_start = Instant::now();
  // 本文画像は段幅に合わせて解決する
  let body_blocks = super::image_resources::resolve_images(body_blocks, ctx.body_col_width.to_pt(), image_resources)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  let stage_start = Instant::now();
  let pages = {
    let _span = debug_span!("break_pages", region = "body").entered();
    typeset::break_pages(
      body_blocks,
      ctx.text_width,
      &ctx.body_geometry,
      &typeset::KnuthPlassBreaker,
      ctx.style.text.alignment,
    )
  };
  info!(
    body_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文のページ分割が完了しました"
  );
  return Ok(BodyLayout { pages, headings });
}
