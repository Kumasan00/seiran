//! ページ分割後に確定する値（ページ番号・総ページ数）の解決機構
//!
//! ページ番号・総ページ数はページ分割（`typeset::breaking::break_pages`）が終わるまで確定しない。目次と
//! running content（ヘッダー・フッター）はこの確定値をそれぞれ異なる時点で必要とするため、
//! その順序制約を型で表す 2 段階に分ける。
//!
//! - Stage 1 [`BodyPageValues`]: 本文ページ列からしか構築できない（目次構築の引数型）
//! - Stage 2 [`PageLabels`]: `finalize`（前付けページ列確定）後にしか得られない（running の引数型）
//!
//! lowering の pass1/pass2（`LayoutNode::Ref` → `resolve_refs`）と同じ「確定後に別走査で解決する」
//! 流儀を型に昇格させたもの。

use config::PageNumbering;
use model::AnchorMark;

/// ページ index/count（`usize`）を番号レンダリング用の `u32` に変換する。
///
/// ページ数が `u32::MAX` に達することはない前提。
fn page_num(n: usize) -> u32 { return u32::try_from(n).expect("ページ数は u32 に収まる前提"); }

/// 本文ページ分割後にしか構築できない確定値（目次構築の引数型）
///
/// 見出しの本文内ページ index とページ番号スタイルを保持する。目次のページラベルは本文の
/// break 完了時点ですでに確定している（本文ページ番号は前付け長に不依存 = R1）ため、
/// このステージだけで [`Self::body_page_label`] によるレンダリングが可能。
pub(super) struct BodyPageValues {
  /// 見出し → 本文内ページ index（文書順）
  heading_pages: Vec<usize>,
  /// 本文ページの総数
  body_page_count: usize,
  /// ページ番号のスタイル設定（クローン所有。借用にするとライフタイムが波及するため）
  numbering: PageNumbering,
}

impl BodyPageValues {
  /// 本文ページ列とページ番号スタイルから [`BodyPageValues`] を構築する。
  ///
  /// `pdf_gen::build_destination_index` と同型のアンカー走査で、各ページの `AnchorMark::Heading`
  /// アンカーだけを文書順に拾う（`AnchorMark::Label` は無視）。
  pub(super) fn from_body_pages(body_pages: &[model::Page], numbering: &PageNumbering) -> Self {
    let mut heading_pages = Vec::new();
    for (page_index, page) in body_pages.iter().enumerate() {
      for anchor in &page.anchors {
        if matches!(anchor.mark, AnchorMark::Heading { .. }) {
          heading_pages.push(page_index);
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
  pub(super) fn heading_pages(&self) -> &[usize] { return &self.heading_pages; }

  /// 本文内ページ index（0 起点）から、本文の番号スタイル（1 起点）でレンダリングしたラベルを返す。
  ///
  /// 目次のページラベル用。前付けのラベル体系（[`Self::finalize`]）とは独立に、本文スタイルだけで
  /// 確定できる。
  pub(super) fn body_page_label(&self, body_page_index: usize) -> String {
    return self.numbering.body.render(page_num(body_page_index) + 1);
  }

  /// 前付けページ列が確定した後、物理ページ順の `({page}, {pages})` ラベル列 [`PageLabels`] を返す。
  ///
  /// 前付け（index `< front_pages.len()`）は `numbering.front_matter`（既定ローマ数字）で 1 から、
  /// 本文はそれ以降を `numbering.body`（既定算用数字）で 1 から振り直す。`{pages}` は同じリージョンの
  /// 総数を同じスタイルでレンダリングしたもの。素の `usize` でなく前付けページ列そのものを引数に取る
  /// ことで、「連結前の前付けページ列が確定していること」を型で要求する（0 や本文数の誤渡しを防ぐ）。
  pub(super) fn finalize(self, front_pages: &[model::Page]) -> PageLabels {
    let front_count = front_pages.len();
    let body_count = self.body_page_count;
    let total = front_count + body_count;
    let mut labels = Vec::with_capacity(total);
    for index in 0..total {
      if index < front_count {
        let page = self.numbering.front_matter.render(page_num(index) + 1);
        let pages = self.numbering.front_matter.render(page_num(front_count));
        labels.push((page, pages));
      } else {
        let body_index = index - front_count;
        let page = self.numbering.body.render(page_num(body_index) + 1);
        let pages = self.numbering.body.render(page_num(body_count));
        labels.push((page, pages));
      }
    }
    return PageLabels { labels };
  }
}

/// 前付けページ列確定後にしか得られない、物理ページ順の `({page}, {pages})` ラベル列（running の引数型）
pub(super) struct PageLabels {
  labels: Vec<(String, String)>,
}

impl PageLabels {
  /// ラベル総数（物理ページ総数と一致するはず）。`build_pages` の `debug_assert` 用。
  pub(super) fn len(&self) -> usize { return self.labels.len(); }

  /// `layout::RunningContentSpec::page_numbers` へ渡すため、所有権ごと `Vec` に変換する。
  pub(super) fn into_vec(self) -> Vec<(String, String)> { return self.labels; }
}

#[cfg(test)]
mod tests {
  use config::PageNumbering;
  use model::{AnchorMark, Page, PlacedAnchor};

  use super::BodyPageValues;

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
            x: model::Length::ZERO,
            y: model::Length::ZERO,
          };
        })
        .collect(),
      links: Vec::new(),
    };
  }

  #[test]
  fn from_body_pages_picks_heading_anchors_in_order() {
    // Arrange — page0 に見出し 1 つ、page1 に Label（無視）+ 見出し 1 つ
    let pages = vec![
      page_with_anchors(vec![AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }]),
      page_with_anchors(vec![
        AnchorMark::Label("tab:1".to_string()),
        AnchorMark::Heading {
          key: "heading:1".to_string(),
          label: None,
        },
      ]),
    ];

    // Act
    let page_values = BodyPageValues::from_body_pages(&pages, &PageNumbering::default());

    // Assert — 見出しアンカーのページ index だけを文書順に拾う（Label は無視）
    assert_eq!(page_values.heading_pages(), &[0, 1]);
  }

  #[test]
  fn body_page_label_renders_with_body_style() {
    // Arrange — 既定は前付け=ローマ小文字 / 本文=算用数字
    let page_values = BodyPageValues::from_body_pages(&[], &PageNumbering::default());

    // Act
    let label = page_values.body_page_label(0);

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
  fn finalize_without_front_matter_is_plain_arabic() {
    // 前付けが無ければ全ページが本文系列（算用数字 1 から）= 従来挙動
    let body_pages = vec![
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
      page_with_anchors(vec![]),
    ];
    let page_values = BodyPageValues::from_body_pages(&body_pages, &PageNumbering::default());

    let labels = page_values.finalize(&[]).into_vec();

    assert_eq!(labels[0].0, "1");
    assert_eq!(labels[2], ("3".to_string(), "3".to_string()));
  }
}
