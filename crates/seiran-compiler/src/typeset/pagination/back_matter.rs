//! 段 4 — 後付けパス（巻末索引をページ分割する）
//!
//! どの機能をどの順に置くかだけを持ち、索引の中身は [`crate::typeset::pagination::index`] が持つ。

use tracing::debug_span;

use crate::typeset::{
  boxes::Page,
  breaking::{Overflow, break_pages},
  error::ElementOverflow,
  pagination::{
    context::{BodyPageFacts, TypesetContext},
    index,
  },
};

/// 巻末索引を生成してページ分割する。
///
/// `\index` が 1 個もなければ空ページ列を返す。
///
/// # Errors
///
/// 後付けの行分割の寸法の計算が長さの値域を超えたら、その要素の起点を [`ElementOverflow`] で返す。
pub(super) fn paginate(
  ctx: &TypesetContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> Result<(Vec<Page>, Vec<Overflow>), ElementOverflow> {
  let back_blocks = index::build_index_blocks(ctx, body_pages, facts)?;
  if back_blocks.is_empty() {
    return Ok((Vec::new(), Vec::new()));
  }
  let (pages, overflows) = {
    let _span = debug_span!("break_pages", matter = "back").entered();
    break_pages(back_blocks, ctx.geometry.text_width(), ctx.geometry.back_geometry(), &ctx.breaker)?
  };
  return Ok((pages, overflows));
}
