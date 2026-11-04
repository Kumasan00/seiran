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
/// 索引の寸法の計算が長さの値域を超えたら [`ElementOverflow`] を返す（索引は現状 `build_blocks` を通らないので
/// 返らないが、後付けの経路は前付けと同じ型で揃える）。
#[expect(
  clippy::unnecessary_wraps,
  reason = "索引ブロックの寸法計算（index::build_index_blocks）の演算子を try_* へ移す後続の Task が ElementOverflow を返す入口。前付けと同じ型で揃えておく"
)]
pub(super) fn paginate(
  ctx: &TypesetContext<'_>,
  body_pages: &mut [Page],
  facts: &BodyPageFacts,
) -> Result<(Vec<Page>, Vec<Overflow>), ElementOverflow> {
  let back_blocks = index::build_index_blocks(ctx, body_pages, facts);
  if back_blocks.is_empty() {
    return Ok((Vec::new(), Vec::new()));
  }
  let (pages, overflows) = {
    let _span = debug_span!("break_pages", matter = "back").entered();
    break_pages(back_blocks, ctx.geometry.text_width(), ctx.geometry.back_geometry(), &ctx.breaker)
  };
  return Ok((pages, overflows));
}
