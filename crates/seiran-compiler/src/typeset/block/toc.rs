//! 目次（table of contents）ブロックの生成パス

use crate::{
  document::{FontKind, HeadingLevel},
  length::Length,
  semantics::HeadingKey,
  style::Style,
  typeset::{
    block::Measurer,
    boxes::{AnchorId, Block, HBox, Line, LineLink, LinkTarget, PositionedBox},
    font::FontSystem,
    lowering::TextStyle,
  },
};

/// 目次生成に必要なプリミティブ設定。
#[derive(Debug, Clone)]
pub(crate) struct TocSpec {
  /// 目次の見出し文字列（例: `"Contents"`）
  pub title: String,
  /// 見出し文字列の書体
  pub title_style: TextStyle,
  /// 見出しとエントリ群の間の縦アキ（pt）
  pub title_bottom_margin: Length,
  /// エントリ本文・ページ番号・リーダーの書体
  pub entry_style: TextStyle,
  /// 見出しレベルの深さ 1 段ごとに加える左インデント（pt）
  pub indent_per_level: Length,
  /// リーダー単位文字列（`None` でリーダー無し）。残り幅いっぱいに反復する
  pub leader: Option<String>,
  /// ページ番号を表示するか
  pub show_page_numbers: bool,
  /// 本文幅（pt）。ページ番号の右端揃えの基準
  pub text_width: Length,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  pub line_height_factor: f32,
  /// 目次ブロック全体の下余白（pt）
  pub bottom_margin: Length,
}

/// 1 目次エントリの入力
#[derive(Debug, Clone)]
pub(crate) struct TocEntryInput {
  /// 見出しレベル（インデントの深さに使う）
  pub level: HeadingLevel,
  /// 書式化済みの見出し番号（空なら番号なし）
  pub number: String,
  /// 見出しタイトル（プレーンテキスト）
  pub title_plain: String,
  /// 表示するページ番号ラベル
  pub page_label: String,
  /// 対応見出しの暗黙 destination キー（内部リンクの行き先）
  pub link_key: HeadingKey,
}

/// スタイルから目次生成用の [`TocSpec`] を組み立てる。
///
/// 目次見出しの書体は文書の節見出しスタイル（[`crate::document::HeadingLevel::Section`]）に揃える。
pub(crate) fn build_toc_spec(style: &Style, text_width: Length) -> TocSpec {
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
pub(crate) fn build_toc_blocks(spec: &TocSpec, entries: &[TocEntryInput], resources: &FontSystem<'_>) -> Vec<Block> {
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

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ・深さ）
#[derive(Default)]
struct LineAccum {
  /// 配置済みボックス列
  boxes: Vec<PositionedBox>,
  /// 行の高さ（ベースラインより上）
  height: Length,
  /// 行の深さ（ベースラインより下）
  depth: Length,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  fn place(&mut self, hboxes: Vec<HBox>, x_start: Length) -> Length {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      self.boxes.push(PositionedBox {
        content: hbox.content,
        x,
        dy: Length::ZERO,
        width: hbox.width,
      });
      x += hbox.width;
    }
    return x;
  }

  /// 累積した内容を `Line`（段落最終行扱い）に確定する
  fn into_line(self, links: Vec<LineLink>) -> Line {
    return Line {
      boxes: self.boxes,
      height: self.height,
      depth: self.depth,
      is_last: true,
      links,
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }
}

/// テキストを左端（x=0）からシェーピングして単一行に組む（見出し行用）
fn compose_left_line(measurer: &mut Measurer<'_>, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(measurer.shape_text(text, style), Length::ZERO);
  return acc.into_line(Vec::new());
}

/// 1 エントリを「番号＋タイトル …リーダー… ページ番号（右寄せ）」の単一行に組む
fn compose_entry_line(measurer: &mut Measurer<'_>, spec: &TocSpec, entry: &TocEntryInput) -> Line {
  let indent = spec.indent_per_level * f32::from(entry.level.depth());
  let label = entry_label(&entry.number, &entry.title_plain);

  let mut acc = LineAccum::default();
  let left_end = acc.place(measurer.shape_text(&label, spec.entry_style), indent);

  let mut right_edge = left_end;
  if spec.show_page_numbers {
    let page_boxes = measurer.shape_text(&entry.page_label, spec.entry_style);
    let page_width: Length = page_boxes.iter().map(|b| return b.width).sum();
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
  let unit_width: Length = measurer.shape_text(unit, style).iter().map(|b| return b.width).sum();
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
  let leader_width: Length = leader_boxes.iter().map(|b| return b.width).sum();
  acc.place(leader_boxes, to_x - leader_width);
}

#[cfg(test)]
mod tests {
  use super::{TextStyle, TocEntryInput, TocSpec, entry_label};
  use crate::{
    document::{FontKind, HeadingLevel},
    length::Length,
    semantics::HeadingKey,
    typeset::boxes::{AnchorId, LinkTarget},
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

  fn entry(level: HeadingLevel, number: &str, title: &str, page: &str, key: usize) -> TocEntryInput {
    return TocEntryInput {
      level,
      number: number.to_string(),
      title_plain: title.to_string(),
      page_label: page.to_string(),
      link_key: HeadingKey::new(key),
    };
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
}
