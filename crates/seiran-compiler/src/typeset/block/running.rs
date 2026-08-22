//! ヘッダー・フッター（running header/footer）の配置パス

use tracing::debug;

use crate::{
  document::FontKind,
  length::Length,
  style::{RunningTemplate, RunningValues},
  typeset::{
    block::Measurer,
    boxes::{HBox, Line, Page, PlacedBlock, PositionedBox},
    font::FontSystem,
    lowering::TextStyle,
  },
};

/// ヘッダー・フッター配置に必要なプリミティブ設定
#[derive(Debug, Clone)]
pub(crate) struct RunningContentSpec {
  /// ヘッダー（ページ上端側）のスロット。`None` なら描画しない
  pub header: Option<RunningSlots>,
  /// フッター（ページ下端側）のスロット。`None` なら描画しない
  pub footer: Option<RunningSlots>,
  /// トークン置換に使う文書メタデータ
  pub metadata: RunningMetadata,
  /// 本文幅（pt）。スロットの左／中央／右揃えの基準
  pub text_width: Length,
  /// 各物理ページの `(\{page\} ラベル, \{pages\} ラベル)`。`pages` と同じ長さ・同じ順序。
  pub page_numbers: Vec<(String, String)>,
  /// 先頭ページ（タイトルページ）のヘッダー・フッターを抑止するか
  pub skip_first: bool,
}

/// 1 リージョン（ヘッダーまたはフッター）のスロットと見た目
#[derive(Debug, Clone)]
pub(crate) struct RunningSlots {
  /// 左スロットのテンプレート
  pub left: RunningTemplate,
  /// 中央スロットのテンプレート
  pub center: RunningTemplate,
  /// 右スロットのテンプレート
  pub right: RunningTemplate,
  /// フォント種別
  pub font_kind: FontKind,
  /// フォントサイズ（pt）
  pub font_size: Length,
  /// ベースラインのページ上端からの距離（pt、絶対座標。フッターは呼び出し側で換算済み）
  pub baseline_y: Length,
  /// 区切り線をテキストの下に置くか（`true`: ヘッダー、`false`: フッター）
  pub rule_below: bool,
  /// 区切り線の太さ（pt）。0 のとき線を描画しない
  pub rule_thickness: Length,
  /// テキストと区切り線の間隔（pt）
  pub rule_gap: Length,
  /// 区切り線の色（RGB）。`None` は黒
  pub rule_color: Option<[u8; 3]>,
}

/// トークン置換に使う文書メタデータ（未設定は空文字列）
#[derive(Debug, Clone, Default)]
pub(crate) struct RunningMetadata {
  /// `{title}`
  pub title: String,
  /// `{author}`
  pub author: String,
  /// `{date}`
  pub date: String,
}

/// 各ページにヘッダー・フッターを配置する
pub(crate) fn layout_running_content(pages: &mut [Page], resources: &FontSystem<'_>, spec: &RunningContentSpec) {
  if spec.header.is_none() && spec.footer.is_none() {
    return;
  }
  let mut measurer = Measurer::new(resources, Length::ZERO, 1.0, None, true);
  for (index, page) in pages.iter_mut().enumerate() {
    if spec.skip_first && index == 0 {
      continue;
    }
    let Some((page_label, pages_label)) = spec.page_numbers.get(index) else {
      continue;
    };
    if let Some(slots) = &spec.header {
      page.header = build_region(&mut measurer, slots, spec.text_width, page_label, pages_label, &spec.metadata);
    }
    if let Some(slots) = &spec.footer {
      page.footer = build_region(&mut measurer, slots, spec.text_width, page_label, pages_label, &spec.metadata);
    }
  }
  debug!(page_count = pages.len(), "ヘッダー・フッターを配置しました");
}

/// 1 リージョン分の配置済みブロック（行＋任意の区切り線）を組み立てる
fn build_region(
  measurer: &mut Measurer<'_>,
  slots: &RunningSlots,
  text_width: Length,
  page_label: &str,
  pages_label: &str,
  metadata: &RunningMetadata,
) -> Vec<PlacedBlock> {
  let style = TextStyle {
    font_size: slots.font_size,
    font_kind: slots.font_kind,
    color: None,
  };
  let left = shape_slot(measurer, &slots.left, page_label, pages_label, metadata, style);
  let center = shape_slot(measurer, &slots.center, page_label, pages_label, metadata, style);
  let right = shape_slot(measurer, &slots.right, page_label, pages_label, metadata, style);

  let center_x = (text_width - slot_width(&center)) / 2.0f32;
  let right_x = text_width - slot_width(&right);

  let mut boxes: Vec<PositionedBox> = Vec::new();
  let mut height = Length::ZERO;
  let mut depth = Length::ZERO;
  append_slot(left, Length::ZERO, &mut boxes, &mut height, &mut depth);
  append_slot(center, center_x, &mut boxes, &mut height, &mut depth);
  append_slot(right, right_x, &mut boxes, &mut height, &mut depth);

  if boxes.is_empty() {
    return Vec::new();
  }

  let mut result = Vec::with_capacity(2);
  result.push(PlacedBlock::Line {
    line: Line {
      boxes,
      height,
      depth,
      is_last: true,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    },
    baseline_y: slots.baseline_y,
  });
  if slots.rule_thickness.is_positive() {
    let y = if slots.rule_below {
      slots.baseline_y + depth + slots.rule_gap
    } else {
      slots.baseline_y - height - slots.rule_gap - slots.rule_thickness
    };
    result.push(PlacedBlock::Rule {
      x: Length::ZERO,
      y,
      width: text_width,
      height: slots.rule_thickness,
      color: slots.rule_color,
    });
  }
  return result;
}

/// 1 スロットのテンプレートをトークン置換してシェーピングした `HBox` 列を返す
fn shape_slot(
  measurer: &mut Measurer<'_>,
  template: &RunningTemplate,
  page_label: &str,
  pages_label: &str,
  metadata: &RunningMetadata,
  style: TextStyle,
) -> Vec<HBox> {
  let text = substitute(template, page_label, pages_label, metadata);
  if text.trim().is_empty() {
    return Vec::new();
  }
  return measurer.shape_text(&text, style);
}

/// テンプレート中のトークンを実値へ置換する
fn substitute(template: &RunningTemplate, page_label: &str, pages_label: &str, metadata: &RunningMetadata) -> String {
  return template.expand(RunningValues {
    page: page_label,
    pages: pages_label,
    title: &metadata.title,
    author: &metadata.author,
    date: &metadata.date,
  });
}

/// `HBox` 列の合計幅（pt）を返す
fn slot_width(hboxes: &[HBox]) -> Length { return hboxes.iter().map(|hbox| return hbox.width).sum(); }

/// `HBox` 列を `x_start` から水平に並べて `boxes` へ追加し、行の高さ・深さを更新する
fn append_slot(
  hboxes: Vec<HBox>,
  x_start: Length,
  boxes: &mut Vec<PositionedBox>,
  height: &mut Length,
  depth: &mut Length,
) {
  let mut x = x_start;
  for hbox in hboxes {
    *height = (*height).max(hbox.height);
    *depth = (*depth).max(hbox.depth);
    boxes.push(PositionedBox {
      content: hbox.content,
      x,
      dy: Length::ZERO,
      width: hbox.width,
    });
    x += hbox.width;
  }
}

#[cfg(test)]
mod tests {
  use super::{RunningMetadata, append_slot, slot_width, substitute};
  use crate::{
    length::Length,
    style::RunningTemplate,
    typeset::boxes::{HBox, HBoxContent, PositionedBox},
  };

  /// 幅 `w`（高さ 8 / 深さ 2）の合成ボックスを作るヘルパ
  fn box_of_width(w: Length) -> HBox {
    return HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: w,
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
    };
  }

  #[test]
  fn slot_width_sums_box_widths() {
    // Arrange / Act
    let width = slot_width(&[
      box_of_width(Length::pt(10.0)),
      box_of_width(Length::pt(15.0)),
    ]);

    // Assert
    assert_eq!(width, Length::pt(25.0));
  }

  #[test]
  fn append_slot_positions_boxes_left_to_right() {
    // Arrange
    let mut boxes: Vec<PositionedBox> = Vec::new();
    let mut height = Length::ZERO;
    let mut depth = Length::ZERO;

    // Act
    append_slot(
      vec![
        box_of_width(Length::pt(10.0)),
        box_of_width(Length::pt(15.0)),
      ],
      Length::pt(100.0),
      &mut boxes,
      &mut height,
      &mut depth,
    );

    // Assert
    let xs: Vec<Length> = boxes.iter().map(|b| return b.x).collect();
    assert_eq!(xs, vec![Length::pt(100.0), Length::pt(110.0)]);
    assert_eq!(height, Length::pt(8.0));
    assert_eq!(depth, Length::pt(2.0));
  }

  fn metadata() -> RunningMetadata {
    return RunningMetadata {
      title: "My Title".to_string(),
      author: "Me".to_string(),
      date: "2026-06-14".to_string(),
    };
  }

  #[test]
  fn substitute_replaces_page_and_pages_independently() {
    // Arrange / Act
    let result = substitute(&RunningTemplate::parse("{page} / {pages}"), "3", "12", &metadata());

    // Assert
    assert_eq!(result, "3 / 12");
  }

  #[test]
  fn substitute_supports_roman_front_matter_labels() {
    // Arrange / Act
    let result = substitute(&RunningTemplate::parse("{page} / {pages}"), "ii", "iv", &metadata());

    // Assert
    assert_eq!(result, "ii / iv");
  }

  #[test]
  fn substitute_replaces_metadata_tokens() {
    // Arrange / Act
    let result = substitute(&RunningTemplate::parse("{title} — {author} ({date})"), "1", "1", &metadata());

    // Assert
    assert_eq!(result, "My Title — Me (2026-06-14)");
  }

  #[test]
  fn substitute_unset_metadata_becomes_empty() {
    // Arrange
    let result = substitute(&RunningTemplate::parse("[{title}]"), "1", "1", &RunningMetadata::default());

    // Assert
    assert_eq!(result, "[]");
  }

  #[test]
  fn substitute_leaves_static_text_untouched() {
    // Arrange / Act
    let result = substitute(&RunningTemplate::parse("Confidential"), "5", "9", &metadata());

    // Assert
    assert_eq!(result, "Confidential");
  }
}
