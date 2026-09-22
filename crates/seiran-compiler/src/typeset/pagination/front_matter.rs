//! 段 3 — 前付けパス（タイトルページ → 目次の順にブロックを積んでページ分割する）
//!
//! どの機能をどの順に置くかだけを持ち、目次の中身（エントリの絞り込み・style の投影・行組み立て）は
//! [`crate::typeset::pagination::toc`] が所有する。

use tracing::{debug, debug_span};

use crate::typeset::{
  boxes::{Block, Page},
  boxing::{BlockBuildInputs, build_blocks},
  breaking::{FootnoteOverflow, break_pages},
  image::ImageResources,
  lowering::{TitlePageMetadata, lower_title_page},
  pagination::{
    context::{BodyPageFacts, TypesetContext},
    toc,
  },
};

/// 前付け（タイトルページ・目次）を生成してページ分割する。
///
/// 前付けは常に 1 段組み（`front_geometry`）で、本文（N 段）とは別に分割する。画像ブロックの描画寸法は
/// 本文と同じく `build_blocks` が確定させるので、前付けに画像が現れても未確定の寸法は作られない。
/// タイトルページ→目次の順にブロックを積む。区画の区切りはタイトルページ自身の末尾の強制改ページだけで、
/// 前付けは強制改ページを積まない（前付けの末尾に残っても強制改ページは冪等なので白紙ページを作らない）。
/// 積むブロックが無い（`title_page` / `toc` がともに無効、またはタイトルページの中身も目次エントリも
/// 無い）なら空ページ列を返す。
///
/// 脚注のはみ出し記録（#382）はページ列と一緒に返す。前付けは生成ブロックだけで組むので実際には
/// 常に空だが、「空のはずだ」という非局所な不変条件を主張せず素通しする。
pub(super) fn typeset_front_matter(
  ctx: &TypesetContext<'_>,
  facts: &BodyPageFacts,
  images: &ImageResources,
) -> (Vec<Page>, Vec<FootnoteOverflow>) {
  let mut front_blocks: Vec<Block> = Vec::new();

  if ctx.style.title_page.enabled {
    let title_metadata = TitlePageMetadata {
      title: ctx.config.document.title.clone(),
      author: ctx.config.document.author.clone(),
      date: ctx.config.document.date.clone(),
    };
    let title_nodes = lower_title_page(&title_metadata, &ctx.style.title_page);
    if !title_nodes.is_empty() {
      let _span = debug_span!("build_blocks", region = "title").entered();
      front_blocks.extend(build_blocks(
        title_nodes,
        &BlockBuildInputs {
          resources: ctx.resources,
          images,
          column_width: ctx.geometry.text_width(),
          default_font_size: ctx.style.text.font_size,
          line_height_factor: ctx.style.text.line_height_factor,
          // タイトルページはハイフネーションしない
          language: None,
          punctuation_spacing: ctx.style.text.punctuation_spacing,
        },
      ));
      debug!("タイトルページを生成");
    }
  }

  if ctx.style.toc.enabled {
    front_blocks.extend(toc::build_toc_blocks(ctx, facts));
  }

  if front_blocks.is_empty() {
    return (Vec::new(), Vec::new());
  }

  let (pages, overflows) = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_pages(
      front_blocks,
      ctx.geometry.text_width(),
      ctx.geometry.front_geometry(),
      &ctx.breaker,
      ctx.style.text.alignment,
    )
  };
  return (pages, overflows);
}
