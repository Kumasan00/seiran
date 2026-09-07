//! 段 4 — 後付けパス（巻末索引をページ分割する）
//!
//! どの機能をどの順に置くかだけを持ち、索引の中身（出現箇所の集約・並び順・区分・ページ番号の
//! 表示方針・style の投影）は [`crate::typeset::pagination::index`] が所有する。

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
/// `\index` が 1 個もなければ空ページ列を返す。索引ブロックの生成は出現ページへ内部リンクの
/// 到達先アンカーを事後追加する（`body_pages` の破壊的更新）。
///
/// 脚注のはみ出し記録（#382）はページ列と一緒に返す。後付けは生成ブロックだけで組むので実際には
/// 常に空だが、「空のはずだ」という非局所な不変条件を主張せず素通しする。
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
    let _span = debug_span!("break_pages", region = "back").entered();
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
