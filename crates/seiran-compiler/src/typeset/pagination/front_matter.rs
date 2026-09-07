//! 段 3 — 前付けパス（タイトルページ → 目次の順にブロックを積んでページ分割する）
//!
//! どの機能をどの順に置くかだけを持ち、目次の中身（エントリの絞り込み・style の投影・行組み立て）は
//! [`crate::typeset::pagination::toc`] が所有する。

use tracing::{debug, debug_span};

use crate::typeset::{
  boxes::{Block, Page},
  boxing::build_blocks,
  breaking::{FootnoteOverflow, break_pages},
  lowering::{TitlePageMetadata, lower_title_page},
  pagination::{
    context::{BodyPageFacts, TypesetContext},
    toc,
  },
};

/// 前付け（タイトルページ・目次）を生成してページ分割する。
///
/// 前付けは常に 1 段組み（`front_geometry`）で、本文（N 段）とは別に分割する。
/// タイトルページ→目次の順にブロックを組み立て、末尾の強制改ページ（本文との境界用）は
/// 空ページを作らないよう分割前に取り除く。`title_page` / `toc` がともに無効・目次エントリが
/// 空なら空ページ列を返す。
///
/// 脚注のはみ出し記録（#382）はページ列と一緒に返す。前付けは生成ブロックだけで組むので実際には
/// 常に空だが、「空のはずだ」という非局所な不変条件を主張せず素通しする。
pub(super) fn typeset_front_matter(
  ctx: &TypesetContext<'_>,
  facts: &BodyPageFacts,
) -> (Vec<Page>, Vec<FootnoteOverflow>) {
  let mut front_blocks: Vec<Block> = Vec::new();

  if ctx.style.title_page.enabled {
    let title_metadata = TitlePageMetadata {
      title: ctx.config.document.title.clone(),
      author: ctx.config.document.author.clone(),
      date: ctx.config.document.date.clone(),
    };
    let title_nodes = lower_title_page(&title_metadata, &ctx.style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      // タイトルページはハイフネーションしない
      front_blocks.extend(build_blocks(
        title_nodes,
        ctx.resources,
        ctx.style.text.font_size,
        ctx.style.text.line_height_factor,
        None,
        ctx.style.text.punctuation_spacing,
      ));
    }
    debug!("タイトルページを生成");
  }

  if ctx.style.toc.enabled {
    let toc_blocks = toc::build_toc_blocks(ctx, facts);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      front_blocks.push(Block::force_break());
    }
  }

  if front_blocks.last().is_some_and(Block::is_force_break) {
    front_blocks.pop();
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
