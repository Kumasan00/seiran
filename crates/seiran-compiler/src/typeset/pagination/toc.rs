//! 目次（table of contents）の生成 — 見出しの絞り込み・ページラベル解決・style 投影・行組み立て
//!
//! 段 3（前付け）から呼ばれる。ページ分割で見出しのページ番号が確定した後に走るので、
//! 入力は [`BodyPageFacts`]（本文の見出し記録 + ページ値）で足りる。
//! 前付けの構成（タイトルページとの順序・改ページ）は呼び出し元 `front_matter` が持つ。

use tracing::debug;

use crate::{
  document::{FontKind, HeadingLevel},
  length::Length,
  semantics::HeadingKey,
  style::{Style, TocStyle},
  typeset::{
    boxes::{AnchorId, Block, Line, LineLink, LinkTarget},
    boxing::{LineAccum, Measurer, row_width},
    font::FontSystem,
    lowering::{HeadingRecord, TextStyle},
    pagination::{
      context::{BodyPageFacts, TypesetContext},
      page_values::BodyPageValues,
    },
  },
};

/// 目次生成に必要なプリミティブ設定。
#[derive(Debug, Clone)]
struct TocSpec {
  /// 目次の見出し文字列（例: `"Contents"`）
  title: String,
  /// 見出し文字列の書体
  title_style: TextStyle,
  /// 見出しとエントリ群の間の縦アキ（pt）
  title_bottom_margin: Length,
  /// エントリ本文・ページ番号・リーダーの書体
  entry_style: TextStyle,
  /// 見出しレベルの深さ 1 段ごとに加える左インデント（pt）
  indent_per_level: Length,
  /// リーダー単位文字列（`None` でリーダー無し）。残り幅いっぱいに反復する
  leader: Option<String>,
  /// ページ番号を表示するか
  show_page_numbers: bool,
  /// 本文幅（pt）。ページ番号の右端揃えの基準
  text_width: Length,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  line_height_factor: f32,
  /// 目次ブロック全体の下余白（pt）
  bottom_margin: Length,
}

/// 1 目次エントリ
#[derive(Debug, Clone)]
struct TocEntry {
  /// 見出しレベル（インデントの深さに使う）
  level: HeadingLevel,
  /// 書式化済みの見出し番号（空なら番号なし）
  number: String,
  /// 見出しタイトル（プレーンテキスト）
  title_plain: String,
  /// 表示するページ番号ラベル
  page_label: String,
  /// 対応見出しの暗黙 destination キー（内部リンクの行き先）
  link_key: HeadingKey,
}

/// 目次の計測済みブロック列を組み立てる。
///
/// 見出しの絞り込み（`style.toc.max_depth`）・ページラベルの解決・style の投影・行組み立てまでを
/// この 1 操作に閉じる。目次に載る見出しが 1 つも無ければ空の `Vec` を返す。
#[must_use]
pub(super) fn build_toc_blocks(ctx: &TypesetContext<'_>, facts: &BodyPageFacts) -> Vec<Block> {
  let entries = collect_toc_entries(&facts.headings, &facts.page_values, &ctx.style.toc);
  let spec = build_toc_spec(ctx.style, ctx.geometry.text_width());
  let blocks = compose_blocks(&spec, &entries, ctx.resources);
  if !blocks.is_empty() {
    debug!(toc_entry_count = entries.len(), "目次を生成");
  }
  return blocks;
}

/// 見出しと本文内ページ index から目次エントリを組み立てる。
///
/// `max_depth` 以上の見出しは除外し、本文の番号スタイルでページラベルを作る。
fn collect_toc_entries(headings: &[HeadingRecord], page_values: &BodyPageValues, toc: &TocStyle) -> Vec<TocEntry> {
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
      return TocEntry {
        level: info.level,
        number: info.number.clone(),
        title_plain: info.title_plain.clone(),
        page_label: page_values.body_page_label(page_index),
        link_key: HeadingKey::new(info.index),
      };
    })
    .collect();
}

/// スタイルから目次生成用の [`TocSpec`] を組み立てる。
///
/// 目次見出しの書体は文書の節見出しスタイル（[`crate::document::HeadingLevel::Section`]）に揃える。
fn build_toc_spec(style: &Style, text_width: Length) -> TocSpec {
  let toc = &style.toc;
  let title_heading = style.heading(HeadingLevel::Section);
  return TocSpec {
    title: toc.title.clone(),
    title_style: TextStyle {
      font_size: title_heading.font_size,
      font_kind: title_heading.font_kind,
      color: None,
    },
    title_bottom_margin: title_heading.bottom_margin,
    entry_style: TextStyle {
      font_size: toc.font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
    indent_per_level: toc.indent_per_level,
    leader: toc.leader.clone(),
    show_page_numbers: toc.show_page_numbers,
    text_width,
    line_height_factor: style.text.line_height_factor,
    bottom_margin: toc.bottom_margin,
  };
}

/// 目次エントリ列を計測済みのブロック列に変換する
#[must_use]
fn compose_blocks(spec: &TocSpec, entries: &[TocEntry], resources: &FontSystem<'_>) -> Vec<Block> {
  if entries.is_empty() {
    return Vec::new();
  }
  let mut measurer = Measurer::new(resources, Length::ZERO, 1.0, None, true);
  let mut blocks: Vec<Block> = Vec::new();

  blocks.push(Block::ComposedLine {
    line: compose_left_line(&mut measurer, &spec.title, spec.title_style),
    leading: spec.title_style.font_size * spec.line_height_factor,
  });
  if spec.title_bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.title_bottom_margin));
  }

  let entry_leading = spec.entry_style.font_size * spec.line_height_factor;
  for entry in entries {
    blocks.push(Block::ComposedLine {
      line: compose_entry_line(&mut measurer, spec, entry),
      leading: entry_leading,
    });
  }

  if spec.bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.bottom_margin));
  }
  return blocks;
}

/// テキストを左端（x=0）からシェーピングして単一行に組む（見出し行用）
fn compose_left_line(measurer: &mut Measurer<'_>, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(measurer.shape_text(text, style), Length::ZERO);
  return acc.into_line(Vec::new());
}

/// 1 エントリを「番号＋タイトル …リーダー… ページ番号（右寄せ）」の単一行に組む
fn compose_entry_line(measurer: &mut Measurer<'_>, spec: &TocSpec, entry: &TocEntry) -> Line {
  let indent = spec.indent_per_level * f32::from(entry.level.depth());
  let label = entry_label(&entry.number, &entry.title_plain);

  let mut acc = LineAccum::default();
  let left_end = acc.place(measurer.shape_text(&label, spec.entry_style), indent);

  let mut right_edge = left_end;
  if spec.show_page_numbers {
    let page_boxes = measurer.shape_text(&entry.page_label, spec.entry_style);
    let page_width = row_width(&page_boxes);
    // ページ番号を右端に揃える（左テキストと重なる場合は left_end まで戻す）
    let page_x = (spec.text_width - page_width).max(left_end);
    // リーダーをページ番号側に寄せて充填する
    if let Some(unit) = &spec.leader {
      fill_leader(measurer, unit, spec.entry_style, left_end, page_x, &mut acc);
    }
    acc.place(page_boxes, page_x);
    right_edge = spec.text_width;
  }

  let links = vec![LineLink {
    target: LinkTarget::Internal(AnchorId::Heading(entry.link_key)),
    x0: indent,
    x1: right_edge,
  }];
  return acc.into_line(links);
}

/// 「番号 タイトル」のラベル文字列を組む（番号・タイトルの空を考慮）
fn entry_label(number: &str, title_plain: &str) -> String {
  if number.is_empty() {
    return title_plain.to_string();
  }
  if title_plain.is_empty() {
    return number.to_string();
  }
  return format!("{number} {title_plain}");
}

/// `from_x` から `to_x` の間をリーダー単位文字列の反復で充填する（ページ番号側に右寄せ）
fn fill_leader(
  measurer: &mut Measurer<'_>,
  unit: &str,
  style: TextStyle,
  from_x: Length,
  to_x: Length,
  acc: &mut LineAccum,
) {
  let available = to_x - from_x;
  if !available.is_positive() {
    return;
  }
  let unit_width = row_width(&measurer.shape_text(unit, style));
  if !unit_width.is_positive() {
    return;
  }
  #[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "`available` / `unit_width` はここまでのガードで非負で、切り捨ては収まる本数を出すための意図した丸め"
  )]
  let count = available.ratio(unit_width).floor() as usize;
  if count == 0 {
    return;
  }
  let leader_boxes = measurer.shape_text(&unit.repeat(count), style);
  let leader_width = row_width(&leader_boxes);
  acc.place(leader_boxes, to_x - leader_width);
}

#[cfg(test)]
mod tests {
  use super::{BodyPageValues, HeadingRecord, TextStyle, TocEntry, TocSpec, collect_toc_entries, entry_label};
  use crate::{
    document::{FontKind, HeadingLevel},
    length::Length,
    semantics::HeadingKey,
    style::{PageNumbering, TocStyle},
    typeset::boxes::{AnchorId, AnchorMark, LinkTarget, Page, PlacedAnchor},
  };

  fn spec() -> TocSpec {
    return TocSpec {
      title: "Contents".to_string(),
      title_style: TextStyle {
        font_size: Length::pt(16.0),
        font_kind: FontKind::SerifBold,
        color: None,
      },
      title_bottom_margin: Length::pt(10.0),
      entry_style: TextStyle {
        font_size: Length::pt(12.0),
        font_kind: FontKind::Serif,
        color: None,
      },
      indent_per_level: Length::pt(12.0),
      leader: Some(".".to_string()),
      show_page_numbers: true,
      text_width: Length::pt(400.0),
      line_height_factor: 1.2,
      bottom_margin: Length::pt(8.0),
    };
  }

  fn entry(level: HeadingLevel, number: &str, title: &str, page: &str, key: usize) -> TocEntry {
    return TocEntry {
      level,
      number: number.to_string(),
      title_plain: title.to_string(),
      page_label: page.to_string(),
      link_key: HeadingKey::new(key),
    };
  }

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
  fn entry_label_combines_number_and_title() {
    assert_eq!(entry_label("1.2", "Intro"), "1.2 Intro");
    assert_eq!(entry_label("", "Intro"), "Intro");
    assert_eq!(entry_label("1.2", ""), "1.2");
  }

  #[test]
  fn spec_and_entry_constructors_are_consistent() {
    let s = spec();
    let e = entry(HeadingLevel::Section, "1.1", "Basics", "3", 1);

    assert!(s.show_page_numbers);
    assert_eq!(s.leader.as_deref(), Some("."));
    assert_eq!(e.link_key, HeadingKey::new(1));
    assert!(
      matches!(LinkTarget::Internal(AnchorId::Heading(e.link_key)), LinkTarget::Internal(k) if k == AnchorId::Heading(HeadingKey::new(1)))
    );
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
