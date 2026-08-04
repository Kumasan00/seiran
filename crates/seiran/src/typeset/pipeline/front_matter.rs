//! 前付け（タイトルページ・目次）パス（lowering → 計測 → 改行・改ページ）

use font::FontSystem;
use model::Length;
use tracing::{debug, debug_span};

use crate::typeset::{
  block::{TocEntryInput, build_blocks, build_toc_blocks, build_toc_spec},
  breaking::{LineBreaker, PageGeometry, break_pages},
  layout::{Block, Page},
  lowering::{TitlePageMetadata, lower_title_page},
};

/// `layout_front_matter` に渡す入力。
pub struct FrontMatterInput<'a> {
  /// タイトルページのメタデータ（title/author/date）の出所となる文書設定
  pub config: &'a config::Config,
  /// 前付けの書式を決めるスタイル設定
  pub style: &'a config::Style,
  /// シェイプ・メトリクス取得の窓口
  pub resources: &'a FontSystem<'a>,
  /// 前付けの版面幅（pt）
  pub text_width: Length,
  /// 前付けのページジオメトリ（常に 1 段組み）
  pub geometry: &'a PageGeometry,
  /// 使用する行分割アルゴリズム
  pub breaker: &'a dyn LineBreaker,
}

/// 前付け（タイトルページ・目次）を生成してページ分割する。
///
/// タイトルページ→目次の順にブロックを組み立て、末尾の強制改ページ（本文との境界用）は
/// 空ページを作らないよう分割前に取り除く。`toc_entries` が空なら目次ブロックは作らない
/// （`title_page` / `toc` がともに無効・目次エントリが空なら空ページ列を返す）。
#[must_use]
pub fn layout_front_matter(input: &FrontMatterInput<'_>, toc_entries: &[TocEntryInput]) -> Vec<Page> {
  let mut front_blocks: Vec<Block> = Vec::new();

  if input.style.title_page.enabled {
    let title_metadata = TitlePageMetadata {
      title: input.config.document.title.clone(),
      author: input.config.document.author.clone(),
      date: input.config.document.date.clone(),
    };
    let title_nodes = lower_title_page(&title_metadata, &input.style.title_page);
    {
      let _span = debug_span!("build_blocks", region = "title").entered();
      // タイトルページはハイフネーションしない
      front_blocks.extend(build_blocks(
        title_nodes,
        input.resources,
        input.style.text.font_size,
        input.style.text.line_height_factor,
        None,
        input.style.text.punctuation_spacing,
      ));
    }
    debug!("タイトルページを生成しました");
  }

  if input.style.toc.enabled {
    let spec = build_toc_spec(input.style, input.text_width);
    let toc_blocks = build_toc_blocks(&spec, toc_entries, input.resources);
    if !toc_blocks.is_empty() {
      front_blocks.extend(toc_blocks);
      front_blocks.push(Block::force_break());
      debug!(toc_entry_count = toc_entries.len(), "目次を生成しました");
    }
  }

  if front_blocks.last().is_some_and(Block::is_force_break) {
    front_blocks.pop();
  }
  if front_blocks.is_empty() {
    return Vec::new();
  }

  let _span = debug_span!("break_pages", region = "front").entered();
  return break_pages(front_blocks, input.text_width, input.geometry, input.breaker, input.style.text.alignment);
}
