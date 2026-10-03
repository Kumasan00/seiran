//! 段 4 — 後付けパス（巻末索引をページ分割する）
//!
//! どの機能をどの順に置くかだけを持ち、索引の中身は [`crate::typeset::pagination::index`] が持つ。

use tracing::debug_span;

use crate::typeset::{
  boxes::Page,
  breaking::{FootnoteOverflow, break_pages},
  pagination::{
    context::{BodyPageFacts, TypesetContext},
    index,
  },
};

/// 巻末索引を生成してページ分割する。
///
/// `\index` が 1 個もなければ空ページ列を返す。
pub(super) fn typeset_back_matter(
  ctx: &TypesetContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> (Vec<Page>, Vec<FootnoteOverflow>) {
  let back_blocks = index::build_index_blocks(ctx, body_pages, facts);
  if back_blocks.is_empty() {
    return (Vec::new(), Vec::new());
  }
  let (pages, overflows) = {
    let _span = debug_span!("break_pages", matter = "back").entered();
    break_pages(
      back_blocks,
      ctx.geometry.text_width(),
      ctx.geometry.back_geometry(),
      &ctx.breaker,
      ctx.style.text.alignment,
    )
  };
  return (pages, overflows);
}
