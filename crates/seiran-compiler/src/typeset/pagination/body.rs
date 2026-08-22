//! 本文パス（lowering → 計測 → 画像サイズ確定 → 改行・改ページ）と、その反復制御

use std::time::Instant;

use tracing::{debug, debug_span};

use crate::{
  semantics::SemanticDocument,
  style::FootnoteNumbering,
  typeset::{
    block::build_blocks,
    boxes::Page,
    breaking::{FootnoteOverflow, break_pages},
    error::TypesetError,
    image::{ImageResources, resolve_images},
    lowering::{HeadingRecord, LoweringContext, lower_sources_with_headings},
    pagination::{context::TypesetContext, elapsed_ms, footnote_numbering},
  },
};

/// 本文パス 1 回ぶんの出力。
#[derive(Debug)]
pub(super) struct BodyLayout {
  /// 確定した本文ページ列
  pub(super) pages: Vec<Page>,
  /// 目次・しおり用の見出し情報（文書順）
  pub(super) headings: Vec<HeadingRecord>,
  /// このパスで収まらなかった脚注の記録（#382、`pages` の中での page index 基準）。
  /// ページ単位採番の不動点反復では収束したパスの `BodyLayout` だけが返るので、
  /// 途中のパスで検出したぶんはここで自然に捨てられる（同じ警告が重複しない）
  pub(super) overflows: Vec<FootnoteOverflow>,
}

/// 本文を組版し、確定ページ列と見出し記録を返す。
///
/// ページ単位の脚注採番では、不動点まで本文パスを反復する。
///
/// # Errors
///
/// 画像解決、脚注採番のいずれかに失敗した場合にエラーを返す（ラベル・`\ref` 解決は
/// `semantics::analyze` が上流で既に完了しているため、ここでは失敗しない）。
pub(super) fn typeset_body(
  ctx: &TypesetContext<'_>,
  document: &SemanticDocument,
  images: &ImageResources,
) -> Result<BodyLayout, TypesetError> {
  let run_pass = |footnote_numbers: Option<&[u32]>| return run_body_pass(ctx, document, images, footnote_numbers);
  return match ctx.style.footnote.numbering {
    FootnoteNumbering::Continuous => run_pass(None),
    FootnoteNumbering::PerPage => footnote_numbering::solve_per_page_numbering(&run_pass),
  };
}

/// 本文パスを 1 回通す。
///
/// lowering → `build_blocks` → 画像サイズ確定 → `break_pages` を 1 呼び出しに畳む。
/// `footnote_numbers` は出現順で引く脚注番号の上書き列（ページ単位採番の不動点反復で複数回呼ばれる）。
///
/// # Errors
///
/// 画像サイズの確定に失敗した場合にエラーを返す（lowering は確定済みの事実を読むだけなので失敗しない）。
fn run_body_pass(
  ctx: &TypesetContext<'_>,
  document: &SemanticDocument,
  images: &ImageResources,
  footnote_numbers: Option<&[u32]>,
) -> Result<BodyLayout, TypesetError> {
  let stage_start = Instant::now();
  let mut lowering_ctx =
    LoweringContext::new(ctx.style).with_image_defaults(ctx.config.image.max_dpi, ctx.config.image.downsample);
  if let Some(numbers) = footnote_numbers {
    lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
  }
  let (body_layout_nodes, headings) = lower_sources_with_headings(&lowering_ctx, document);
  debug!(elapsed_ms = elapsed_ms(stage_start), "意味解析の成果物 → LayoutNode への変換が完了しました");

  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    build_blocks(
      body_layout_nodes,
      ctx.resources,
      ctx.style.text.font_size,
      ctx.style.text.line_height_factor,
      ctx.config.document.language.as_deref(),
      ctx.style.text.punctuation_spacing,
    )
  };
  debug!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  // 本文画像は段幅に合わせて解決する
  let stage_start = Instant::now();
  let body_blocks = resolve_images(body_blocks, ctx.body_col_width.to_pt(), images)?;
  debug!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  let stage_start = Instant::now();
  let (pages, overflows) = {
    let _span = debug_span!("break_pages", region = "body").entered();
    break_pages(body_blocks, ctx.text_width, &ctx.body_geometry, &ctx.breaker, ctx.style.text.alignment)
  };
  debug!(
    body_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文のページ分割が完了しました"
  );
  return Ok(BodyLayout {
    pages,
    headings,
    overflows,
  });
}
