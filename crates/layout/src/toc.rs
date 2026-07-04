//! 目次（table of contents）ブロックの生成パス
//!
//! `seiran::build_pdf` が本文を一度ページ分割して各見出しのページ番号を確定した**後**に走る、
//! フォント依存の生成パスです。見出しの意味情報（レベル・番号・タイトル）と確定済みページ
//! ラベルから、各エントリを「番号＋タイトル …リーダー… ページ番号（右寄せ）」の 1 行に組み、
//! [`hlist::Block::ComposedLine`] として返します。
//!
//! `read_style` / `read_config` には依存せず、呼び出し側がプリミティブの [`TocSpec`] /
//! [`TocEntryInput`] を組み立てて渡します（[`crate::running`] と同じ流儀）。各エントリ行には
//! 対応見出しへの内部リンク（[`hlist::LineLink`]）を付与し、PDF 上でクリック可能にします。

use font::{FontMetrics, shaper::HarfRustShapers};
use hlist::{Block, HBox, Line, LineLink, PositionedBox};
use lowering::TextStyle;
use types::{HeadingLevel, LinkTarget};

use crate::Measurer;

/// 目次生成に必要なプリミティブ設定（`read_style` 非依存）
#[derive(Debug, Clone)]
pub struct TocSpec {
  /// 目次の見出し文字列（例: `"Contents"`）
  pub title: String,
  /// 見出し文字列の書体
  pub title_style: TextStyle,
  /// 見出しとエントリ群の間の縦アキ（pt）
  pub title_bottom_margin: f32,
  /// エントリ本文・ページ番号・リーダーの書体
  pub entry_style: TextStyle,
  /// 見出しレベルの深さ 1 段ごとに加える左インデント（pt）
  pub indent_per_level: f32,
  /// リーダー単位文字列（`None` でリーダー無し）。残り幅いっぱいに反復する
  pub leader: Option<String>,
  /// ページ番号を表示するか
  pub show_page_numbers: bool,
  /// 本文幅（pt）。ページ番号の右端揃えの基準
  pub text_width: f32,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  pub line_height_factor: f32,
  /// 目次ブロック全体の下余白（pt）
  pub bottom_margin: f32,
}

/// 1 目次エントリの入力
#[derive(Debug, Clone)]
pub struct TocEntryInput {
  /// 見出しレベル（インデントの深さに使う）
  pub level: HeadingLevel,
  /// 書式化済みの見出し番号（空なら番号なし）
  pub number: String,
  /// 見出しタイトル（プレーンテキスト）
  pub title_plain: String,
  /// 表示するページ番号ラベル
  pub page_label: String,
  /// 対応見出しの暗黙 destination キー（内部リンクの行き先）
  pub link_key: String,
}

/// 目次エントリ列を計測済みのブロック列に変換する
///
/// `entries` が空のときは空ベクタを返す（目次を出さない）。
#[must_use]
pub fn build_toc_blocks(
  spec: &TocSpec,
  entries: &[TocEntryInput],
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
) -> Vec<Block> {
  if entries.is_empty() {
    return Vec::new();
  }
  // default_font_size / line_height_factor は目次のシェーピングでは使わない。
  // 目次項目はハイフネーションしない（本文段落専用・#173）。
  // 和文約物アキ調整は本文と同じく既定で有効にする
  let mut measurer = Measurer::new(shapers, metrics, 0.0, 1.0, None, true);
  let mut blocks: Vec<Block> = Vec::new();

  // 見出し行
  blocks.push(Block::ComposedLine {
    line: compose_left_line(&mut measurer, &spec.title, spec.title_style),
    leading: spec.title_style.font_size * spec.line_height_factor,
  });
  if spec.title_bottom_margin > 0.0 {
    blocks.push(Block::fixed_space(spec.title_bottom_margin));
  }

  // エントリ行
  let entry_leading = spec.entry_style.font_size * spec.line_height_factor;
  for entry in entries {
    blocks.push(Block::ComposedLine {
      line: compose_entry_line(&mut measurer, spec, entry),
      leading: entry_leading,
    });
  }

  if spec.bottom_margin > 0.0 {
    blocks.push(Block::fixed_space(spec.bottom_margin));
  }
  return blocks;
}

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ・深さ）
#[derive(Default)]
struct LineAccum {
  boxes: Vec<PositionedBox>,
  height: f32,
  depth: f32,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  fn place(&mut self, hboxes: Vec<HBox>, x_start: f32) -> f32 {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      self.boxes.push(PositionedBox {
        content: hbox.content,
        x,
        dy: 0.0,
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
    };
  }
}

/// テキストを左端（x=0）からシェーピングして単一行に組む（見出し行用）
fn compose_left_line(measurer: &mut Measurer, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(measurer.shape_text(text, style), 0.0);
  return acc.into_line(Vec::new());
}

/// 1 エントリを「番号＋タイトル …リーダー… ページ番号（右寄せ）」の単一行に組む
fn compose_entry_line(measurer: &mut Measurer, spec: &TocSpec, entry: &TocEntryInput) -> Line {
  let indent = f32::from(entry.level.depth()) * spec.indent_per_level;
  let label = entry_label(&entry.number, &entry.title_plain);

  let mut acc = LineAccum::default();
  // 左: 番号＋タイトル（インデント位置から）
  let left_end = acc.place(measurer.shape_text(&label, spec.entry_style), indent);

  let mut right_edge = left_end;
  if spec.show_page_numbers {
    let page_boxes = measurer.shape_text(&entry.page_label, spec.entry_style);
    let page_width: f32 = page_boxes.iter().map(|b| b.width).sum();
    // ページ番号を右端に揃える（左テキストと重なる場合は left_end まで戻す）
    let page_x = (spec.text_width - page_width).max(left_end);
    // リーダーをページ番号側に寄せて充填する
    if let Some(unit) = &spec.leader {
      fill_leader(measurer, unit, spec.entry_style, left_end, page_x, &mut acc);
    }
    acc.place(page_boxes, page_x);
    right_edge = spec.text_width;
  }

  // 行全体を見出しへの内部リンクにする
  let links = vec![LineLink {
    target: LinkTarget::Internal(entry.link_key.clone()),
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
fn fill_leader(measurer: &mut Measurer, unit: &str, style: TextStyle, from_x: f32, to_x: f32, acc: &mut LineAccum) {
  let available = to_x - from_x;
  if available <= 0.0 {
    return;
  }
  let unit_width: f32 = measurer.shape_text(unit, style).iter().map(|b| b.width).sum();
  if unit_width <= 0.0 {
    return;
  }
  // available / unit_width はここまでのガードで非負。truncation は意図どおり（収まる本数）。
  #[allow(clippy::cast_sign_loss)]
  let count = (available / unit_width).floor() as usize;
  if count == 0 {
    return;
  }
  let leader_boxes = measurer.shape_text(&unit.repeat(count), style);
  let leader_width: f32 = leader_boxes.iter().map(|b| b.width).sum();
  acc.place(leader_boxes, to_x - leader_width);
}

#[cfg(test)]
mod tests {
  use types::{FontKind, HeadingLevel, LinkTarget};

  use super::{TocEntryInput, TocSpec, entry_label};
  use crate::TextStyle;

  fn spec() -> TocSpec {
    return TocSpec {
      title: "Contents".to_string(),
      title_style: TextStyle {
        font_size: 16.0,
        font_kind: FontKind::SerifBold,
        color: None,
      },
      title_bottom_margin: 10.0,
      entry_style: TextStyle {
        font_size: 12.0,
        font_kind: FontKind::Serif,
        color: None,
      },
      indent_per_level: 12.0,
      leader: Some(".".to_string()),
      show_page_numbers: true,
      text_width: 400.0,
      line_height_factor: 1.2,
      bottom_margin: 8.0,
    };
  }

  fn entry(level: HeadingLevel, number: &str, title: &str, page: &str, key: &str) -> TocEntryInput {
    return TocEntryInput {
      level,
      number: number.to_string(),
      title_plain: title.to_string(),
      page_label: page.to_string(),
      link_key: key.to_string(),
    };
  }

  #[test]
  fn entry_label_combines_number_and_title() {
    assert_eq!(entry_label("1.2", "Intro"), "1.2 Intro");
    assert_eq!(entry_label("", "Intro"), "Intro");
    assert_eq!(entry_label("1.2", ""), "1.2");
  }

  // フォントを使う build_toc_blocks の配置検証（リーダー充填・右端揃え・リンク span）は
  // フォント依存のため `seiran` の結合テストで確認する。ここでは純粋ロジックのみ検証する。
  #[test]
  fn spec_and_entry_constructors_are_consistent() {
    // Arrange / Act
    let s = spec();
    let e = entry(HeadingLevel::Section, "1.1", "Basics", "3", "heading:1");

    // Assert — 入力の素通しを確認（型整合のスモーク）
    assert!(s.show_page_numbers);
    assert_eq!(s.leader.as_deref(), Some("."));
    assert_eq!(e.link_key, "heading:1");
    assert!(matches!(LinkTarget::Internal(e.link_key.clone()), LinkTarget::Internal(k) if k == "heading:1"));
  }
}
