//! phase 1: 本文の block 構築とページ分割
//!
//! Document IR → `LayoutNode` → `build_blocks` → 画像サイズ確定 → `break_pages` を 1 本の
//! パスとして通す。脚注の採番方式でこのパスの回し方だけが変わり（通し採番は 1 回、ページ単位
//! 採番は [`super::footnote_numbering`] の不動点反復）、パスの中身は同一。

use std::time::Instant;

use pdf_gen::ImageSet;
use tracing::{debug_span, info};
use typeset::LoweringContext;

use super::{ParsedProject, compile::CompileContext, elapsed_ms, footnote_numbering, wrap_lowering_error};

/// 本文パス 1 回ぶんの出力。
#[derive(Debug)]
pub(super) struct BodyLayout {
  /// 確定した本文ページ列
  pub(super) pages: Vec<model::Page>,
  /// 目次・しおり用の見出し情報（文書順）
  pub(super) headings: Vec<typeset::HeadingRecord>,
}

/// 本文を組版し、確定ページ列と見出し記録を返す（phase graph の 1 段目）。
///
/// 脚注の採番方式で本文パスの回し方が変わる（他は一切変わらない）。通し採番は番号がページに
/// 依存しないので 1 回だけ通し、ページ単位採番のときだけ [`super::footnote_numbering`] の
/// 専用 solver が番号を与えて複数回呼ぶ。
///
/// # Errors
///
/// lowering・画像サイズ確定に失敗した場合、またはページ単位採番が収束しなかった場合にエラーを返す。
pub(super) fn typeset_body(
  ctx: &CompileContext<'_>,
  parsed_project: &ParsedProject,
  image_set: &ImageSet,
) -> miette::Result<BodyLayout> {
  let groups = parsed_project.lowering_groups();
  let run_pass = |footnote_numbers: Option<&[u32]>| {
    return run_body_pass(ctx, parsed_project, &groups, image_set, footnote_numbers);
  };
  return match ctx.style.footnote.numbering {
    config::FootnoteNumbering::Continuous => run_pass(None),
    config::FootnoteNumbering::PerPage => footnote_numbering::solve_per_page_numbering(&run_pass),
  };
}

/// 本文パスを 1 回通す（lowering → `build_blocks` → 画像サイズ確定 → `break_pages`）。
///
/// `footnote_numbers` は脚注の表示番号の上書きマップ（出現 index 引き）。通し採番では `None` を
/// 渡し、上書きマップを一切通さない経路になる。
fn run_body_pass(
  ctx: &CompileContext<'_>,
  parsed_project: &ParsedProject,
  groups: &[typeset::SourceGroup<'_>],
  image_set: &ImageSet,
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
      ctx.shapers,
      ctx.metrics,
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
  // 本文画像は段幅に合わせて解決する（段抜き＝全幅フロートは将来検討）。
  let body_blocks = pdf_gen::resolve_images(body_blocks, ctx.body_col_width.to_pt(), image_set)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  // 本文は前付け（タイトルページ・目次）と別系列で 1 から番号付けするため、得られる本文内ページ
  // 番号が最終値になる（前付けの長さに不依存。break_pages は純粋）。
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
