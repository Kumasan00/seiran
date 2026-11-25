//! 柱の `{chapter}` / `{section}` — 本文ページごとに、最初の本文ブロックが属する章・節を決める

use crate::{
  document::HeadingLevel,
  typeset::{
    boxes::{AnchorId, Page},
    lowering::HeadingRecord,
  },
};

/// ページの柱に出す（章, 節）の見出し記録
type Heads<'h> = (Option<&'h HeadingRecord>, Option<&'h HeadingRecord>);

/// 本文ページごとに、最初の本文ブロック（読み順）が属する（章, 節）を返す
///
/// 基準はページ最初の本文ブロックで着地した見出し（[`Page::opening_headings`]）、無ければ前のページまでに着地した
/// 最後の見出し。ページ途中の見出しはそのページを切り替えない。本文ブロックの無い（繰越脚注だけの）ページは、その脚注の
/// 呼出元のうち文書順で最初のものの直前の見出しを基準にする。`headings` は `HeadingKey` の順に並ぶ
/// （`lowering::lower_sources_with_headings` が保証する）。
pub(super) fn page_heads<'h>(body_pages: &[Page], headings: &'h [HeadingRecord]) -> Vec<Heads<'h>> {
  let after = heads_after_each_heading(headings);
  let mut passed: Option<usize> = None;
  let mut result = Vec::with_capacity(body_pages.len());
  for page in body_pages {
    let basis = if page.blocks.is_empty() {
      let Some(first_call) = page.footnotes.iter().map(|footnote| return footnote.index).min() else {
        unreachable!("本文ブロックも脚注も無いページは改ページが排出しない（PageDraft::has_content）")
      };
      headings.iter().rposition(|record| return record.footnote_start <= first_call)
    } else {
      page.opening_headings.last().map(|key| return key.index()).or(passed)
    };
    result.push(basis.map_or((None, None), |index| {
      let Some(&heads) = after.get(index) else {
        unreachable!("見出しアンカーのキーは lowering が見出し記録と同じ文書順で振る: {index}")
      };
      return heads;
    }));
    passed = page
      .anchors
      .iter()
      .filter_map(|anchor| match anchor.id {
        AnchorId::Heading(key) => return Some(key.index()),
        _ => return None,
      })
      .max()
      .or(passed);
  }
  return result;
}

/// 見出し記録ごとに、その見出しの直後の（章, 節）を返す
///
/// 新しい章は節を終え、部は章と節を終える。小節以下は変えない。書誌の見出し（`node` が `None`）以降は本文外で、
/// 章・節とも無い。
fn heads_after_each_heading(headings: &[HeadingRecord]) -> Vec<Heads<'_>> {
  let mut current: Heads<'_> = (None, None);
  let mut result = Vec::with_capacity(headings.len());
  for record in headings {
    if record.node.is_none() {
      current = (None, None);
    } else {
      current = match record.level {
        HeadingLevel::Part => (None, None),
        HeadingLevel::Chapter => (Some(record), None),
        HeadingLevel::Section => (current.0, Some(record)),
        HeadingLevel::Subsection | HeadingLevel::Paragraph | HeadingLevel::Subparagraph => current,
      };
    }
    result.push(current);
  }
  return result;
}

#[cfg(test)]
mod tests {
  use super::page_heads;
  use crate::{
    document::{HeadingLevel, NodeId},
    length::Length,
    semantics::HeadingKey,
    source::SourceId,
    typeset::{
      boxes::Page,
      lowering::HeadingRecord,
      test_support::{PageBuilder, rule_block},
    },
  };

  /// 本文の見出し記録（`index` 番目の見出し、題目なし）
  fn record(index: usize, level: HeadingLevel) -> HeadingRecord {
    return HeadingRecord {
      node: Some(NodeId::for_test(SourceId::new(0), u32::try_from(index).expect("テストの見出し数は u32 に収まる"))),
      index,
      level,
      number: String::new(),
      title_plain: String::new(),
      toc_label: Vec::new(),
      header_title: Vec::new(),
      footer_title: Vec::new(),
      footnote_start: 0,
    };
  }

  /// 脚注 `footnote_start` 番以降を従える本文の節見出しの記録
  fn section(index: usize, footnote_start: u32) -> HeadingRecord {
    return HeadingRecord {
      footnote_start,
      ..record(index, HeadingLevel::Section)
    };
  }

  /// 書誌の見出し記録（生成物なので `node` を持たない）
  fn bibliography(index: usize) -> HeadingRecord {
    return HeadingRecord {
      node: None,
      ..record(index, HeadingLevel::Section)
    };
  }

  /// 本文ブロックを 1 つ持つページのビルダ
  fn page() -> PageBuilder {
    return PageBuilder::new().block(rule_block(Length::ZERO, Length::ZERO, Length::pt(1.0), Length::pt(1.0), None));
  }

  /// ページごとの（章, 節）を見出し記録の index で返す
  fn heads(pages: &[Page], headings: &[HeadingRecord]) -> Vec<(Option<usize>, Option<usize>)> {
    return page_heads(pages, headings)
      .into_iter()
      .map(|(chapter, section)| {
        return (chapter.map(|record| return record.index), section.map(|record| return record.index));
      })
      .collect();
  }

  /// 各ページの先頭に見出しを 1 つずつ置いたページ列
  fn one_heading_per_page(count: usize) -> Vec<Page> {
    return (0..count).map(|index| return page().opening_heading(HeadingKey::new(index)).build()).collect();
  }

  #[test]
  fn heading_opening_a_page_applies_to_that_page() {
    let headings = vec![
      record(0, HeadingLevel::Chapter),
      record(1, HeadingLevel::Section),
    ];
    let pages = vec![
      page()
        .opening_heading(HeadingKey::new(0))
        .heading_anchor(HeadingKey::new(1), Length::ZERO, Length::pt(20.0))
        .build(),
      page().build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(Some(0), None), (Some(0), Some(1))]);
  }

  #[test]
  fn mid_page_heading_applies_from_the_next_page() {
    let headings = vec![
      record(0, HeadingLevel::Section),
      record(1, HeadingLevel::Section),
    ];
    let pages = vec![
      page()
        .opening_heading(HeadingKey::new(0))
        .heading_anchor(HeadingKey::new(1), Length::ZERO, Length::pt(20.0))
        .build(),
      page().build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, Some(0)), (None, Some(1))]);
  }

  #[test]
  fn chapter_ends_the_section_and_part_ends_both() {
    let headings = vec![
      record(0, HeadingLevel::Chapter),
      record(1, HeadingLevel::Section),
      record(2, HeadingLevel::Chapter),
      record(3, HeadingLevel::Section),
      record(4, HeadingLevel::Part),
    ];

    assert_eq!(
      heads(&one_heading_per_page(5), &headings),
      vec![
        (Some(0), None),
        (Some(0), Some(1)),
        (Some(2), None),
        (Some(2), Some(3)),
        (None, None),
      ]
    );
  }

  #[test]
  fn section_without_chapter_is_used() {
    let headings = vec![record(0, HeadingLevel::Section)];
    let pages = vec![
      page().build(),
      page().opening_heading(HeadingKey::new(0)).build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, None), (None, Some(0))]);
  }

  #[test]
  fn lower_levels_do_not_change_the_heads() {
    let headings = vec![
      record(0, HeadingLevel::Chapter),
      record(1, HeadingLevel::Section),
      record(2, HeadingLevel::Subsection),
      record(3, HeadingLevel::Paragraph),
      record(4, HeadingLevel::Subparagraph),
    ];

    assert_eq!(
      heads(&one_heading_per_page(5), &headings),
      vec![
        (Some(0), None),
        (Some(0), Some(1)),
        (Some(0), Some(1)),
        (Some(0), Some(1)),
        (Some(0), Some(1)),
      ]
    );
  }

  #[test]
  fn bibliography_pages_have_no_heads() {
    let headings = vec![record(0, HeadingLevel::Section), bibliography(1)];
    let pages = vec![
      page()
        .opening_heading(HeadingKey::new(0))
        .heading_anchor(HeadingKey::new(1), Length::ZERO, Length::pt(20.0))
        .build(),
      page().build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, Some(0)), (None, None)]);
  }

  #[test]
  fn footnote_only_page_uses_the_call_heading() {
    let headings = vec![section(0, 0), section(1, 1)];
    let pages = vec![
      page()
        .opening_heading(HeadingKey::new(0))
        .heading_anchor(HeadingKey::new(1), Length::ZERO, Length::pt(20.0))
        .build(),
      PageBuilder::new().continued_footnote(0).build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, Some(0)), (None, Some(0))]);
  }

  #[test]
  fn footnote_only_page_takes_the_earliest_call() {
    let headings = vec![section(0, 0), section(1, 1)];
    let pages = vec![
      page()
        .opening_heading(HeadingKey::new(0))
        .heading_anchor(HeadingKey::new(1), Length::ZERO, Length::pt(20.0))
        .build(),
      PageBuilder::new().continued_footnote(2).continued_footnote(0).build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, Some(0)), (None, Some(0))]);
  }

  #[test]
  fn footnote_called_before_any_heading_has_no_heads() {
    let headings = vec![section(0, 1)];
    let pages = vec![
      page().build(),
      PageBuilder::new().continued_footnote(0).build(),
    ];

    assert_eq!(heads(&pages, &headings), vec![(None, None), (None, None)]);
  }
}
