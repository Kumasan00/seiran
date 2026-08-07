//! ページ分割後に確定する値（ページ番号・総ページ数）の解決機構
//!
//! 値が確定する順序を型で表すため、2 段階に分ける。
//!
//! - Stage 1 [`BodyPageValues`]: 本文ページ列からしか構築できない（目次構築の引数型）
//! - Stage 2 [`PageLabels`]: `finalize`（前付けページ列確定）後にしか得られない（running の引数型）

use crate::{config::PageNumbering, typeset::AnchorMark};

/// 物理ページ index（0 始まり）。あるページ列（本文単体、または前付け・本文・後付けを
/// 連結する前のリージョン内）における位置を表す。
///
/// 表示用の論理ページ値（[`PageValue`]）とは別の型にして、「何番目のページか」と
/// 「何ページと表示されるか」を取り違えないようにする（両方とも `usize`/`u32` のままだと、
/// 引数を取り違えても型検査を素通りしてしまう）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PageIndex(usize);

impl PageIndex {
  /// 0 始まりの物理 index から構築する。
  pub(super) fn new(index: usize) -> Self { return PageIndex(index); }

  /// 内側の `usize` を返す。
  pub(super) fn get(self) -> usize { return self.0; }
}

/// 領域ごとに 1 から振り直す、文字列化前の論理ページ値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PageValue(u32);

impl PageValue {
  /// 0 始まりの物理 index から 1 始まりの論理値を作る。
  ///
  /// ページ数が `u32::MAX` に達することはない前提。
  fn from_index(index: PageIndex) -> Self {
    return PageValue(u32::try_from(index.get()).expect("ページ数は u32 に収まる前提") + 1);
  }

  /// 0 始まりの物理カウント（総ページ数）から 1 始まりの論理値を作る。
  ///
  /// ページ数が `u32::MAX` に達することはない前提。
  fn from_count(count: usize) -> Self {
    return PageValue(u32::try_from(count).expect("ページ数は u32 に収まる前提"));
  }

  /// 内側の `u32` を返す。
  fn get(self) -> u32 { return self.0; }
}

/// 本文ページ分割後に確定する、目次生成用の値。
///
/// 見出しの本文内ページ index とページ番号スタイルを保持する。
pub(super) struct BodyPageValues {
  /// 見出し → 本文内ページ index（文書順）
  heading_pages: Vec<PageIndex>,
  /// 本文ページの総数
  body_page_count: usize,
  /// ページ番号のスタイル設定（クローン所有。借用にするとライフタイムが波及するため）
  numbering: PageNumbering,
}

impl BodyPageValues {
  /// 本文ページ列とページ番号スタイルから構築する。
  pub(super) fn from_body_pages(body_pages: &[crate::typeset::Page], numbering: &PageNumbering) -> Self {
    let mut heading_pages = Vec::new();
    for (page_index, page) in body_pages.iter().enumerate() {
      for anchor in &page.anchors {
        if matches!(anchor.mark, AnchorMark::Heading { .. }) {
          heading_pages.push(PageIndex::new(page_index));
        }
      }
    }
    return Self {
      heading_pages,
      body_page_count: body_pages.len(),
      numbering: numbering.clone(),
    };
  }

  /// 見出し → 本文内ページ index の列（文書順）を返す。
  pub(super) fn heading_pages(&self) -> &[PageIndex] { return &self.heading_pages; }

  /// 索引ページを本文領域の通し番号へ加算する。
  pub(super) fn with_back_matter(mut self, back_pages: &[crate::typeset::Page]) -> Self {
    self.body_page_count += back_pages.len();
    return self;
  }

  /// 本文内ページ index を本文の番号スタイルでレンダリングする。
  pub(super) fn body_page_label(&self, body_page_index: PageIndex) -> String {
    return self.numbering.body.render(PageValue::from_index(body_page_index).get());
  }

  /// 物理ページ順の `({page}, {pages})` ラベル列を確定する。
  ///
  /// 前付けと本文はそれぞれ 1 から番号を振り直す。
  pub(super) fn finalize(self, front_pages: &[crate::typeset::Page]) -> PageLabels {
    let front_count = front_pages.len();
    let body_count = self.body_page_count;
    let total = front_count + body_count;
    let mut labels = Vec::with_capacity(total);
    for raw_index in 0..total {
      let index = PageIndex::new(raw_index);
      if raw_index < front_count {
        let page = self.numbering.front_matter.render(PageValue::from_index(index).get());
        let pages = self.numbering.front_matter.render(PageValue::from_count(front_count).get());
        labels.push((page, pages));
      } else {
        let body_index = PageIndex::new(raw_index - front_count);
        let page = self.numbering.body.render(PageValue::from_index(body_index).get());
        let pages = self.numbering.body.render(PageValue::from_count(body_count).get());
        labels.push((page, pages));
      }
    }
    return PageLabels { labels };
  }
}

/// 物理ページ順の `({page}, {pages})` ラベル列。
pub(super) struct PageLabels {
  /// 物理ページ順の `({page}, {pages})` ラベル
  labels: Vec<(String, String)>,
}

impl PageLabels {
  /// ラベル総数を返す。
  pub(super) fn len(&self) -> usize { return self.labels.len(); }

  /// 所有権ごとラベル列へ変換する。
  pub(super) fn into_vec(self) -> Vec<(String, String)> { return self.labels; }
}

#[cfg(test)]
mod tests {
  use super::{BodyPageValues, PageIndex};
  use crate::{
    config::PageNumbering,
    semantics::{HeadingKey, LabelId},
    typeset::{AnchorMark, Page, PlacedAnchor},
  };

  /// 指定マークのアンカーだけを持つページを作るヘルパ
  fn page_with_anchors(marks: Vec<AnchorMark>) -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: marks
        .into_iter()
        .map(|mark| {
          return PlacedAnchor {
            mark,
            x: crate::length::Length::ZERO,
            y: crate::length::Length::ZERO,
          };
        })
        .collect(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  #[test]
  fn from_body_pages_picks_heading_anchors_in_order() {
    // Arrange — page0 に見出し 1 つ、page1 に Label（無視）+ 見出し 1 つ
    let pages = vec![
      page_with_anchors(vec![AnchorMark::Heading {
        key: HeadingKey::new(0),
        label: None,
      }]),
      page_with_anchors(vec![
        AnchorMark::Label(LabelId::new("tab:1")),
        AnchorMark::Heading {
          key: HeadingKey::new(1),
          label: None,
        },
      ]),
    ];

    // Act
    let page_values = BodyPageValues::from_body_pages(&pages, &PageNumbering::default());

    // Assert — 見出しアンカーのページ index だけを文書順に拾う（Label は無視）
    assert_eq!(page_values.heading_pages(), &[PageIndex::new(0), PageIndex::new(1)]);
  }

  #[test]
  fn body_page_label_renders_with_body_style() {
    // Arrange — 既定は前付け=ローマ小文字 / 本文=算用数字
    let page_values = BodyPageValues::from_body_pages(&[], &PageNumbering::default());

    // Act
    let label = page_values.body_page_label(PageIndex::new(0));

    // Assert — 本文スタイル（算用数字）でレンダリングされ、前付けのローマ数字にはならない
    assert_eq!(label, "1");
  }

  #[test]
  fn finalize_roman_front_arabic_body() {
    // Arrange — 既定（前付け=ローマ小文字 / 本文=算用）。前付け 2 ページ、本文 3 ページ
    let front_pages = vec![page_with_anchors(vec![]), page_with_anchors(vec![])];
    let body_pages = vec![
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
    ];
    let page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    // Act
    let labels = page_values.finalize(&front_pages).into_vec();

    // Assert — 前付けは i, ii（総数 ii）、本文は 1..3（総数 3）でリージョン別に振り直す
    assert_eq!(labels[0], ("i".to_string(), "ii".to_string()));
    assert_eq!(labels[1], ("ii".to_string(), "ii".to_string()));
    assert_eq!(labels[2], ("1".to_string(), "3".to_string()));
    assert_eq!(labels[4], ("3".to_string(), "3".to_string()));
  }

  #[test]
  fn with_back_matter_extends_body_region_numbering() {
    // Arrange — 前付け 1 ページ・本文 2 ページ・索引（back matter）1 ページ
    let front_pages = vec![page_with_anchors(vec![])];
    let body_pages = vec![page_with_anchors(vec![]), page_with_anchors(vec![])];
    let back_pages = vec![page_with_anchors(vec![])];
    let page_values =
      BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default()).with_back_matter(&back_pages);

    // Act
    let labels = page_values.finalize(&front_pages).into_vec();

    // Assert — 索引ページも本文領域の通し番号（算用数字）に含まれ、総数は本文+索引の 3
    assert_eq!(labels.len(), 4);
    assert_eq!(labels[1], ("1".to_string(), "3".to_string()));
    assert_eq!(labels[2], ("2".to_string(), "3".to_string()));
    assert_eq!(labels[3], ("3".to_string(), "3".to_string()), "索引ページも本文の通し番号を継続するはず");
  }

  #[test]
  fn with_back_matter_is_no_op_when_no_index() {
    // Arrange
    let front_pages = vec![page_with_anchors(vec![])];
    let body_pages = vec![page_with_anchors(vec![]), page_with_anchors(vec![])];
    let labels_with_empty_back = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default())
      .with_back_matter(&[])
      .finalize(&front_pages)
      .into_vec();
    let labels_without_call = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default())
      .finalize(&front_pages)
      .into_vec();

    // Act / Assert
    assert_eq!(labels_with_empty_back, labels_without_call);
  }

  #[test]
  fn finalize_without_front_matter_is_plain_arabic() {
    // Arrange
    let body_pages = vec![
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
    ];
    let page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    // Act
    let labels = page_values.finalize(&[]).into_vec();

    // Assert
    assert_eq!(labels[0].0, "1");
    assert_eq!(labels[2], ("3".to_string(), "3".to_string()));
  }
}
