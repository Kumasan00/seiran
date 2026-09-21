//! 本文パス（lowering → 計測 → 改行・改ページ）と、その反復制御

use tracing::debug_span;

use crate::{
  semantics::SemanticDocument,
  style::FootnoteNumbering,
  typeset::{
    boxes::Page,
    boxing::{BlockBuildInputs, build_blocks},
    breaking::{FootnoteOverflow, break_pages},
    error::TypesetError,
    image::ImageResources,
    lowering::{HeadingRecord, LoweringContext, lower_sources_with_headings},
    pagination::{context::TypesetContext, footnote_numbering},
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
/// 脚注採番に失敗した場合にエラーを返す（画像の描画寸法は `build_blocks` が確定させ、
/// ラベル・`\ref` 解決は `semantics::analyze` が上流で既に完了しているため、ここでは失敗しない）。
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
/// lowering → `build_blocks` → `break_pages` を 1 呼び出しに畳む。
/// `footnote_numbers` は出現順で引く脚注番号の上書き列（ページ単位採番の不動点反復で複数回呼ばれる）。
///
/// # Errors
///
/// この経路に失敗要因は無い（画像の描画寸法は `build_blocks` が確定させ、lowering は確定済みの
/// 事実を読むだけ）。`Result` は `footnote_numbering::solve_per_page_numbering` が要求する
/// コールバック型に合わせて残している。
#[expect(
  clippy::unnecessary_wraps,
  reason = "ページ単位採番の不動点反復 footnote_numbering::solve_per_page_numbering が \
            Result を返すコールバックを取るため、この経路の Result は呼び出し側の型の都合で残る"
)]
fn run_body_pass(
  ctx: &TypesetContext<'_>,
  document: &SemanticDocument,
  images: &ImageResources,
  footnote_numbers: Option<&[u32]>,
) -> Result<BodyLayout, TypesetError> {
  let mut lowering_ctx =
    LoweringContext::new(ctx.style).with_image_defaults(ctx.config.image.max_dpi, ctx.config.image.downsample);
  if let Some(numbers) = footnote_numbers {
    lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
  }
  let (body_layout_nodes, headings) = lower_sources_with_headings(&lowering_ctx, document);

  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    build_blocks(
      body_layout_nodes,
      &BlockBuildInputs {
        resources: ctx.resources,
        images,
        column_width: ctx.geometry.body_column_width(),
        default_font_size: ctx.style.text.font_size,
        line_height_factor: ctx.style.text.line_height_factor,
        language: ctx.config.document.language.as_deref(),
        punctuation_spacing: ctx.style.text.punctuation_spacing,
      },
    )
  };

  let (pages, overflows) = {
    let _span = debug_span!("break_pages", region = "body").entered();
    break_pages(
      body_blocks,
      ctx.geometry.text_width(),
      ctx.geometry.body_geometry(),
      &ctx.breaker,
      ctx.style.text.alignment,
    )
  };
  return Ok(BodyLayout {
    pages,
    headings,
    overflows,
  });
}
