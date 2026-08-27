//! 前付け（タイトルページ・目次）パス（目次エントリの組み立て → 計測 → 改行・改ページ）

use std::time::Instant;

use tracing::{debug, debug_span};

use crate::{
  semantics::HeadingKey,
  style::TocStyle,
  typeset::{
    boxes::{Block, Page},
    boxing::{TocEntryInput, build_blocks, build_toc_blocks, build_toc_spec},
    breaking::{FootnoteOverflow, break_pages},
    lowering::{HeadingRecord, TitlePageMetadata, lower_title_page},
    pagination::{
      context::{BodyPageFacts, TypesetContext},
      elapsed_ms,
      page_values::BodyPageValues,
    },
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
  let stage_start = Instant::now();
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
    debug!("タイトルページを生成しました");
  }

  if ctx.style.toc.enabled {
    let toc_entries = collect_toc_entries(&facts.headings, &facts.page_values, &ctx.style.toc);
    let spec = build_toc_spec(ctx.style, ctx.text_width);
    let toc_blocks = build_toc_blocks(&spec, &toc_entries, ctx.resources);
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
    return (Vec::new(), Vec::new());
  }

  let (pages, overflows) = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_pages(front_blocks, ctx.text_width, &ctx.front_geometry, &ctx.breaker, ctx.style.text.alignment)
  };
  debug!(
    front_page_count = pages.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "前付けのページ分割が完了しました"
  );
  return (pages, overflows);
}

/// 見出しと本文内ページ index から目次エントリを組み立てる。
///
/// `max_depth` 以上の見出しは除外し、本文の番号スタイルでページラベルを作る。
fn collect_toc_entries(headings: &[HeadingRecord], page_values: &BodyPageValues, toc: &TocStyle) -> Vec<TocEntryInput> {
  let heading_pages = page_values.heading_pages();
  if headings.len() != heading_pages.len() {
    unreachable!(
      "lowering は見出し記録 1 件につき Heading アンカーを 1 個だけ出し、break_pages は全アンカーを \
       いずれかの本文ページへ載せるので数が一致する: headings={} pages={}",
      headings.len(),
      heading_pages.len()
    )
  }
  return headings
    .iter()
    .zip(heading_pages.iter().copied())
    .filter(|(info, _)| return u32::from(info.level.depth()) < toc.max_depth)
    .map(|(info, page_index)| {
      return TocEntryInput {
        level: info.level,
        number: info.number.clone(),
        title_plain: info.title_plain.clone(),
        page_label: page_values.body_page_label(page_index),
        link_key: HeadingKey::new(info.index),
      };
    })
    .collect();
}

#[cfg(test)]
mod tests {
  use super::{BodyPageValues, HeadingRecord, Page, collect_toc_entries};
  use crate::{
    document::HeadingLevel,
    length::Length,
    semantics::HeadingKey,
    style::{PageNumbering, TocStyle},
    typeset::boxes::{AnchorMark, PlacedAnchor},
  };

  fn heading_record(index: usize, level: HeadingLevel, number: &str, title_plain: &str) -> HeadingRecord {
    return HeadingRecord {
      index,
      level,
      number: number.to_string(),
      title_plain: title_plain.to_string(),
    };
  }

  /// 各ページに 1 つずつ見出しアンカーを持つ本文ページ列から [`BodyPageValues`] を作るヘルパ
  fn body_page_values_with_headings(heading_count: usize) -> BodyPageValues {
    let pages: Vec<Page> = (0..heading_count)
      .map(|index| {
        return Page {
          blocks: Vec::new(),
          header: Vec::new(),
          footer: Vec::new(),
          footnotes: Vec::new(),
          anchors: vec![PlacedAnchor {
            mark: AnchorMark::Heading {
              key: HeadingKey::new(index),
              label: None,
            },
            x: Length::ZERO,
            y: Length::ZERO,
          }],
          links: Vec::new(),
          index_entries: Vec::new(),
          background_color: None,
          content_origin_x: Length::ZERO,
        };
      })
      .collect();
    return BodyPageValues::from_body_pages(&pages, &PageNumbering::default());
  }

  #[test]
  fn collect_toc_entries_filters_by_max_depth_and_renders_page_label() {
    // Arrange — Chapter(深さ1)/Section(深さ2)/Subsection(深さ3)。max_depth=3 は深さ<3 を残す
    let headings = vec![
      heading_record(0, HeadingLevel::Chapter, "1", "Ch"),
      heading_record(1, HeadingLevel::Section, "1.1", "Sec"),
      heading_record(2, HeadingLevel::Subsection, "1.1.1", "Sub"),
    ];
    let page_values = body_page_values_with_headings(3);
    let toc = TocStyle {
      max_depth: 3,
      ..TocStyle::default()
    };

    // Act
    let entries = collect_toc_entries(&headings, &page_values, &toc);

    // Assert — Subsection は除外、ページラベルは本文算用数字、リンクキーは文書順インデックス由来
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].number, "1");
    assert_eq!(entries[0].page_label, "1");
    assert_eq!(entries[0].link_key, HeadingKey::new(0));
    assert_eq!(entries[1].title_plain, "Sec");
    assert_eq!(entries[1].page_label, "2");
    assert_eq!(entries[1].link_key, HeadingKey::new(1));
  }
}
