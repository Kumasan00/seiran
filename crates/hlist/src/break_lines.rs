//! (c) 行分割 — 貪欲法（first-fit）・左揃え（ragged-right）
//!
//! [`LineBreaker`] トレイトの実装として [`GreedyBreaker`] を提供する。
//! box は計測済みの幅を持つため、行分割はフォントに一切触れない純粋計算になる。
//! 将来 Knuth-Plass を導入する際は入出力型を変えずに実装を追加する。

use crate::{
  hitem::HItem,
  line::{Line, PositionedBox},
};

/// 行分割アルゴリズムの抽象
pub trait LineBreaker {
  /// 水平リストを本文幅で行に分割する
  ///
  /// `text_width` は本文幅（pt）。分割点が存在しない場合は overflow を許容する。
  fn break_lines(&self, items: &[HItem], text_width: f32) -> Vec<Line>;
}

/// 貪欲法（first-fit）による行分割
///
/// アイテムを順に詰め、`Box` / `Kern` の追加で本文幅を超えるとき直近の分割可能点
/// （breakable `Glue` または `Penalty { value <= 0 }`）で行を確定する。
/// 行末の breakable glue は破棄され（行末スペース不可視）、折り返し直後の行頭の
/// breakable glue も破棄される。
#[derive(Debug, Clone, Copy, Default)]
pub struct GreedyBreaker;

impl LineBreaker for GreedyBreaker {
  fn break_lines(&self, items: &[HItem], text_width: f32) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    // 現在の行に積んだアイテム
    let mut buffer: Vec<&HItem> = Vec::new();
    // 現在の行の自然幅
    let mut width_so_far = 0.0f32;
    // 直近の分割可能点（buffer 内インデックス）
    let mut last_break: Option<usize> = None;

    for item in items {
      match item {
        HItem::ForcedBreak => {
          lines.push(build_line(&buffer, true));
          buffer.clear();
          width_so_far = 0.0;
          last_break = None;
        },
        HItem::Glue {
          natural, breakable, ..
        } => {
          // 行頭の breakable glue は不可視（折り返し直後・段落頭のスペースを落とす）
          if buffer.is_empty() && *breakable {
            continue;
          }
          buffer.push(item);
          if *breakable {
            last_break = Some(buffer.len() - 1);
          }
          width_so_far += natural;
        },
        HItem::Penalty { value } => {
          buffer.push(item);
          if *value <= 0 {
            last_break = Some(buffer.len() - 1);
          }
        },
        HItem::Box(_) | HItem::Kern(_) => {
          let item_width = item.natural_width();
          if width_so_far + item_width > text_width
            && let Some(break_index) = last_break
          {
            // 分割可能点までで行を確定し、残りを次行へ持ち越す
            lines.push(build_line(&buffer[..break_index], false));
            let carried: Vec<&HItem> = buffer[break_index + 1..].to_vec();
            buffer = carried;
            // 持ち越し先頭の breakable glue は破棄する
            while matches!(
              buffer.first(),
              Some(HItem::Glue {
                breakable: true,
                ..
              })
            ) {
              buffer.remove(0);
            }
            width_so_far = buffer.iter().map(|i| i.natural_width()).sum();
            last_break = None;
          }
          buffer.push(item);
          width_so_far += item_width;
        },
      }
    }

    if !buffer.is_empty() || lines.is_empty() {
      lines.push(build_line(&buffer, true));
    }
    return lines;
  }
}

/// アイテム列から 1 行を組み立てる（左揃え・位置確定）
///
/// 行末の breakable glue は破棄する。`Penalty` は幅を持たないため位置決めに影響しない。
fn build_line(items: &[&HItem], is_last: bool) -> Line {
  // 行末の breakable glue を切り落とす
  let mut end = items.len();
  while end > 0
    && matches!(
      items[end - 1],
      HItem::Glue {
        breakable: true,
        ..
      }
    )
  {
    end -= 1;
  }
  let items = &items[..end];

  let mut boxes: Vec<PositionedBox> = Vec::new();
  let mut x = 0.0f32;
  let mut height = 0.0f32;
  let mut depth = 0.0f32;
  for item in items {
    match item {
      HItem::Box(hbox) => {
        boxes.push(PositionedBox {
          content: hbox.content.clone(),
          x,
          dy: 0.0,
          width: hbox.width,
        });
        x += hbox.width;
        height = height.max(hbox.height);
        depth = depth.max(hbox.depth);
      },
      HItem::Glue { natural, .. } => x += natural,
      HItem::Kern(value) => x += value,
      HItem::Penalty { .. } | HItem::ForcedBreak => {},
    }
  }
  return Line {
    boxes,
    height,
    depth,
    is_last,
  };
}

#[cfg(test)]
mod tests {
  use super::{GreedyBreaker, LineBreaker};
  use crate::hitem::{HBox, HBoxContent, HItem};

  /// テスト用の合成ボックス（幅 10、高さ 8、深さ 2）
  fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 10.0,
        height: 1.0,
      },
      width: 10.0,
      height: 8.0,
      depth: 2.0,
    });
  }

  /// テスト用の breakable glue（幅 5）
  fn space_glue() -> HItem {
    return HItem::Glue {
      natural: 5.0,
      stretch: 0.0,
      shrink: 0.0,
      breakable: true,
    };
  }

  #[test]
  fn breaks_at_glue_when_box_exceeds_width() {
    // Arrange — box(10) glue(5) box(10) glue(5) box(10): text_width=30 では
    // 3 つ目の box(合計幅 40) が収まらず、2 つ目の glue で折り返す
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 30.0);

    // Assert — 1 行目は box 2 つ（行末 glue は破棄）、2 行目は box 1 つ
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2);
    assert!(!lines[0].is_last);
    assert_eq!(lines[1].boxes.len(), 1);
    assert!(lines[1].is_last);
    // 2 行目の box は行頭（x=0）から始まる
    assert!((lines[1].boxes[0].x - 0.0).abs() < f32::EPSILON);
  }

  #[test]
  fn discards_trailing_glue_at_line_end() {
    // Arrange — 行末で破棄された glue は行幅に寄与しない
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 30.0);

    // Assert — 1 行目: box(0..10) glue(10..15) box(15..25)。2 つ目の box の x は 15
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    // 幅が十分なら 1 行に収まり、is_last = true
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].boxes.len(), 2);
    assert!(lines[0].is_last);
  }

  #[test]
  fn breaks_at_zero_penalty_between_boxes() {
    // Arrange — CJK 相当: box Penalty(0) box Penalty(0) box、text_width=25 で
    // 3 つ目の box が収まらず 2 つ目の Penalty で折り返す
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 25.0);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2);
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn never_breaks_at_prohibitive_penalty() {
    // Penalty(i32::MAX) は分割可能点にならず、overflow を許容する
    let items = vec![
      test_box(),
      HItem::Penalty { value: i32::MAX },
      test_box(),
      HItem::Penalty { value: i32::MAX },
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, 25.0);

    assert_eq!(lines.len(), 1, "分割点がなければ overflow 許容: {lines:?}");
    assert_eq!(lines[0].boxes.len(), 3);
  }

  #[test]
  fn forced_break_flushes_line_unconditionally() {
    // ForcedBreak は幅に関係なく行を確定し、is_last = true になる
    let items = vec![test_box(), HItem::ForcedBreak, test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0);

    assert_eq!(lines.len(), 2);
    assert!(lines[0].is_last, "強制改行の行は is_last: {lines:?}");
    assert!(lines[1].is_last);
  }

  #[test]
  fn line_height_and_depth_from_boxes() {
    // 行の height / depth は行内ボックスの最大値
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0);

    assert!((lines[0].height - 8.0).abs() < f32::EPSILON);
    assert!((lines[0].depth - 2.0).abs() < f32::EPSILON);
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    // 空の段落も 1 行（空行）として返す
    let lines = GreedyBreaker.break_lines(&[], 100.0);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
    assert!(lines[0].is_last);
  }

  #[test]
  fn kern_is_not_a_break_opportunity_and_is_kept() {
    // Kern は分割点にならず、行末でも破棄されない（幅に寄与する）
    let items = vec![test_box(), HItem::Kern(5.0), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0);

    assert_eq!(lines.len(), 1);
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn no_line_exceeds_width_when_break_points_exist() {
    // 不変条件: breakable glue で区切られ、各 box が段幅より狭ければ
    // どの行も段幅を超えない（box(10) を glue(5) で 10 個連結、text_width=30）
    let mut items = Vec::new();
    for i in 0..10 {
      if i > 0 {
        items.push(space_glue());
      }
      items.push(test_box());
    }

    let lines = GreedyBreaker.break_lines(&items, 30.0);

    for line in &lines {
      let width = line.boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
      assert!(width <= 30.0 + f32::EPSILON, "行幅 {width} が段幅 30 を超えた: {line:?}");
    }
  }

  #[test]
  fn single_box_wider_than_width_is_not_split() {
    // 分割は機会位置のみ: 段幅より広い単一 box は分割されず 1 行に overflow する
    let wide = HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 50.0,
        height: 1.0,
      },
      width: 50.0,
      height: 8.0,
      depth: 2.0,
    });

    let lines = GreedyBreaker.break_lines(&[wide], 30.0);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!((lines[0].boxes[0].width - 50.0).abs() < f32::EPSILON);
  }
}
