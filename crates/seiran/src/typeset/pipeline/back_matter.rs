//! 後付け（巻末索引）パス（計測 → 改行・改ページ）

use tracing::debug_span;

use crate::{
  font::FontSystem,
  model::Length,
  typeset::{
    block::{IndexEntryInput, build_index_blocks, build_index_spec},
    breaking::{LineBreaker, PageGeometry, break_pages},
    layout::Page,
  },
};

/// `layout_back_matter` に渡す入力。
pub struct BackMatterInput<'a> {
  /// 後付けの書式を決めるスタイル設定
  pub style: &'a crate::config::Style,
  /// シェイプ・メトリクス取得の窓口
  pub resources: &'a FontSystem<'a>,
  /// 後付けの版面幅（pt）
  pub text_width: Length,
  /// 後付けのページジオメトリ
  pub geometry: &'a PageGeometry,
  /// 使用する行分割アルゴリズム
  pub breaker: &'a dyn LineBreaker,
}

/// 巻末索引を生成してページ分割する。
///
/// `entries` が空（`\index` が 1 個もない）なら空ページ列を返す。
#[must_use]
pub fn layout_back_matter(input: &BackMatterInput<'_>, entries: &[IndexEntryInput]) -> Vec<Page> {
  if entries.is_empty() {
    return Vec::new();
  }
  let spec = build_index_spec(input.style);
  let back_blocks = build_index_blocks(&spec, entries, input.resources);
  let _span = debug_span!("break_pages", region = "back").entered();
  return break_pages(back_blocks, input.text_width, input.geometry, input.breaker, input.style.text.alignment);
}
