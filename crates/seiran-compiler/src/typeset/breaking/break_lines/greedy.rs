//! 貪欲法（first-fit）による行分割

use tracing::trace;

use crate::{
  length::Length,
  style::TextAlignment,
  typeset::{
    boxes::{HItem, Line},
    breaking::break_lines::{LineBreaker, OpenLink, build_line},
    observe,
  },
};

/// 貪欲法（first-fit）による行分割
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct GreedyBreaker;

impl LineBreaker for GreedyBreaker {
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut buffer: Vec<&HItem> = Vec::new();
    let mut width_so_far = Length::ZERO;
    let mut last_break: Option<usize> = None;
    // 折り返しをまたいで開いているリンク領域（行間で引き継ぐ）
    let mut open_links: Vec<OpenLink> = Vec::new();

    for item in items {
      match item {
        HItem::ForcedBreak => {
          let line = build_line(&buffer, true, text_width, alignment, &mut open_links, None);
          push_line(&mut lines, line, true, false);
          buffer.clear();
          width_so_far = Length::ZERO;
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
          width_so_far += *natural;
        },
        HItem::Penalty { value } => {
          buffer.push(item);
          if *value <= 0 {
            last_break = Some(buffer.len() - 1);
          }
        },
        HItem::Discretionary { hyphen } => {
          buffer.push(item);
          // ハイフンを足しても本文幅に収まる語中点だけを分割候補にする
          // （折り返すと行末にハイフンが乗るため、右端超過を作らない）。自然幅には寄与しない
          if width_so_far + hyphen.width <= text_width {
            last_break = Some(buffer.len() - 1);
          }
        },
        // リンクマーカー・脚注マーカー・索引マーカーは幅 0・分割不可。行に積むだけで build_line が収集する
        HItem::LinkStart(_) | HItem::LinkEnd | HItem::Footnote { .. } | HItem::IndexMark { .. } => {
          buffer.push(item);
        },
        HItem::Box(_) | HItem::Kern(_) | HItem::FlushRight(_) => {
          let item_width = item.natural_width();
          if width_so_far + item_width > text_width
            && let Some(break_index) = last_break
          {
            // 分割可能点までで行を確定し、残りを次行へ持ち越す。
            // 語中（Discretionary）で折り返すときは行末にハイフンを付す
            let trailing_hyphen = match buffer[break_index] {
              HItem::Discretionary { hyphen } => Some(hyphen),
              _ => None,
            };
            let line =
              build_line(&buffer[..break_index], false, text_width, alignment, &mut open_links, trailing_hyphen);
            push_line(&mut lines, line, false, trailing_hyphen.is_some());
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
            width_so_far = buffer.iter().map(|i| return i.natural_width()).sum();
            last_break = None;
          }
          buffer.push(item);
          width_so_far += item_width;
        },
      }
    }

    if !buffer.is_empty() || lines.is_empty() {
      let line = build_line(&buffer, true, text_width, alignment, &mut open_links, None);
      push_line(&mut lines, line, true, false);
    }
    return lines;
  }
}

/// 確定した行を積み、TRACE へ出す
///
/// `line_index` は段落内の連番（0 起点。強制改行でもリセットしない — 貪欲法は強制改行を段落の途中として
/// 扱うため）。badness は載せない（貪欲法は疎密のコストを評価しないので存在しない値）。
fn push_line(lines: &mut Vec<Line>, line: Line, is_last: bool, hyphen: bool) {
  trace!(
    line_index = lines.len(),
    is_last,
    hyphen,
    width_pt = line.width().to_pt(),
    text = %observe::summarize_line(&line),
    "貪欲法で行を確定しました"
  );
  lines.push(line);
}

#[cfg(test)]
mod tests {
  use super::{GreedyBreaker, LineBreaker};
  use crate::{
    length::Length,
    style::TextAlignment,
    typeset::{
      boxes::{HBox, HBoxContent, HItem},
      breaking::break_lines::test_support::{
        cjk_glue, discretionary, flush_right_box, index_mark, link_target, non_breakable_stretch_glue, space_glue,
        stretch_glue, test_box,
      },
    },
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に（sp 丸め精度内で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 2 つの `Length` が（sp 丸め精度内で）一致するか
  fn close_l(a: Length, b: Length) -> bool { return (a - b).abs() <= Length::from_sp(1); }

  #[test]
  fn breaks_at_glue_when_box_exceeds_width() {
    // Arrange
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2);
    assert!(!lines[0].is_last);
    assert_eq!(lines[1].boxes.len(), 1);
    assert!(lines[1].is_last);
    assert!(close(lines[1].boxes[0].x, 0.0));
  }

  #[test]
  fn discards_trailing_glue_at_line_end() {
    // Arrange
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::RaggedRight);

    // Assert
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].boxes.len(), 2);
    assert!(lines[0].is_last);
  }

  #[test]
  fn index_mark_is_collected_without_affecting_width_or_breaks() {
    // Arrange
    let items = vec![test_box(), index_mark("語", None), test_box()];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].boxes.len(), 2, "index_mark はボックスとして描画されない");
    assert!(close(lines[0].boxes[1].x, 10.0), "index_mark を挟んでも 2 つ目の box の x は不変: {lines:?}");
    assert_eq!(lines[0].index_marks.len(), 1);
    assert_eq!(lines[0].index_marks[0].word, "語");
    assert_eq!(lines[0].index_marks[0].reading, None);
  }

  #[test]
  fn breaks_at_zero_penalty_between_boxes() {
    // Arrange
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2);
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn never_breaks_at_prohibitive_penalty() {
    let items = vec![
      test_box(),
      HItem::Penalty { value: i32::MAX },
      test_box(),
      HItem::Penalty { value: i32::MAX },
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "分割点がなければ overflow 許容: {lines:?}");
    assert_eq!(lines[0].boxes.len(), 3);
  }

  #[test]
  fn forced_break_flushes_line_unconditionally() {
    let items = vec![test_box(), HItem::ForcedBreak, test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 2);
    assert!(lines[0].is_last, "強制改行の行は is_last: {lines:?}");
    assert!(lines[1].is_last);
  }

  #[test]
  fn line_height_and_depth_from_boxes() {
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    assert!(close(lines[0].height, 8.0));
    assert!(close(lines[0].depth, 2.0));
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    let lines = GreedyBreaker.break_lines(&[], Length::pt(100.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
    assert!(lines[0].is_last);
  }

  #[test]
  fn kern_is_not_a_break_opportunity_and_is_kept() {
    let items = vec![test_box(), HItem::Kern(Length::pt(5.0)), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1);
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    // Arrange
    let items = vec![test_box(), space_glue(), flush_right_box(8.0)];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(50.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED box の 2 つ: {lines:?}");
    assert!(close(lines[0].boxes[0].x, 0.0));
    assert!(close(lines[0].boxes[1].x, 42.0), "QED は右端寄せ: {lines:?}");
    assert!(lines[0].is_last);
  }

  #[test]
  fn flush_right_box_wraps_to_next_line_when_it_does_not_fit() {
    // Arrange
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(14.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert!(close(lines[0].boxes[0].x, 0.0));
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED box のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].x, 6.0), "QED は右端寄せ: {lines:?}");
    assert!(lines[1].is_last);
  }

  #[test]
  fn no_line_exceeds_width_when_break_points_exist() {
    let mut items = Vec::new();
    for i in 0..10 {
      if i > 0 {
        items.push(space_glue());
      }
      items.push(test_box());
    }

    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::RaggedRight);

    for line in &lines {
      let width = line.boxes.iter().map(|b| return b.x + b.width).fold(Length::ZERO, Length::max).to_pt();
      assert!(width <= 30.0 + f32::EPSILON, "行幅 {width} が段幅 30 を超えた: {line:?}");
    }
  }

  #[test]
  fn link_markers_collect_single_rect_on_one_line() {
    // Arrange
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].links.len(), 1);
    assert!(close(lines[0].links[0].x0, 0.0));
    assert!(close(lines[0].links[0].x1, 20.0), "{:?}", lines[0].links);
  }

  #[test]
  fn link_spanning_wrap_splits_into_two_rects() {
    // Arrange
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      space_glue(),
      test_box(),
      HItem::LinkEnd,
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(12.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "1 行目に継続中の矩形: {:?}", lines[0].links);
    assert!(close(lines[0].links[0].x1, 10.0));
    assert_eq!(lines[1].links.len(), 1, "2 行目に残りの矩形: {:?}", lines[1].links);
    assert!(close(lines[1].links[0].x0, 0.0));
    assert!(close(lines[1].links[0].x1, 10.0));
  }

  #[test]
  fn line_without_links_has_empty_links() {
    let lines =
      GreedyBreaker.break_lines(&[test_box(), space_glue(), test_box()], Length::pt(100.0), TextAlignment::RaggedRight);

    assert!(lines[0].links.is_empty());
  }

  #[test]
  fn single_box_wider_than_width_is_not_split() {
    let wide = HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(50.0),
      height: pt(8.0),
      depth: pt(2.0),
    });

    let lines = GreedyBreaker.break_lines(&[wide], Length::pt(30.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!(close(lines[0].boxes[0].width, 50.0));
  }

  #[test]
  fn justify_stretches_glue_to_flush_right_edge() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 17.0), "{lines:?}");
    let right_edge = lines[0].boxes[1].x + lines[0].boxes[1].width;
    assert!(close(right_edge, 27.0), "非最終行の右端は版面右端に一致: {lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_last_line() {
    // Arrange
    let items = vec![test_box(), stretch_glue(), test_box()];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 1);
    assert!(lines[0].is_last);
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_line_before_forced_break() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].is_last);
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_stretch_limit() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 17.5), "伸長は能力の上限で止まる: {lines:?}");
  }

  #[test]
  fn justify_shrinks_overfull_line() {
    // Arrange
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(24.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 14.0), "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_shrink_limit() {
    // Arrange
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(23.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 15.0 - 5.0 / 3.0), "収縮は能力の下限で止まる: {lines:?}");
  }

  #[test]
  fn justify_leaves_line_without_stretch_points_ragged() {
    // Arrange
    let items = vec![
      test_box(),
      HItem::Kern(Length::pt(5.0)),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(26.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_moves_link_rects_with_stretched_glue() {
    // Arrange
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::LinkEnd,
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!(close(lines[0].links[0].x0, 0.0));
    assert!(close(lines[0].links[0].x1, 27.0), "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn ragged_right_ignores_stretch_capacity() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_stretches_cjk_zero_width_glue_to_flush_right_edge() {
    // Arrange
    let items = vec![test_box(), cjk_glue(), test_box(), cjk_glue(), test_box()];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(20.3), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 10.3), "{lines:?}");
    let right_edge = lines[0].boxes[1].x + lines[0].boxes[1].width;
    assert!(close(right_edge, 20.3), "和文のみの非最終行の右端は版面右端に一致: {lines:?}");
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    // Arrange
    let items = vec![
      test_box(),
      discretionary(3.0),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 3, "本文 box 2 つ + 行末ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[2].x, 20.0), "{lines:?}");
    assert!(close(lines[0].boxes[2].width, 3.0), "{lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| return b.x + b.width).fold(Length::ZERO, Length::max).to_pt();
    assert!(right_edge <= 25.0 + f32::EPSILON, "ハイフン込みで右端超過なし: {right_edge}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn discretionary_not_used_when_word_fits() {
    let items = vec![test_box(), discretionary(3.0), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "ハイフン箱は付かない: {lines:?}");
  }

  #[test]
  fn discretionary_rejected_when_hyphen_would_overflow() {
    // Arrange
    let items = vec![
      test_box(),
      discretionary(1.0),
      test_box(),
      discretionary(20.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(22.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "box1 + ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[1].width, 1.0), "使われたのは disc1 のハイフン: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
  }

  #[test]
  fn justify_includes_hyphen_width_at_flush_right_edge() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, Length::pt(29.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 16.0), "glue 伸長後の box2: {lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| return b.x + b.width).fold(Length::ZERO, Length::max);
    assert!(close(right_edge, 29.0), "ハイフン込みで右端に揃う: {}", right_edge.to_pt());
  }

  #[test]
  fn cjk_zero_width_glue_breaks_like_zero_penalty() {
    // Arrange
    let penalty_items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];
    let glue_items = vec![test_box(), cjk_glue(), test_box(), cjk_glue(), test_box()];

    // Act
    let penalty_lines = GreedyBreaker.break_lines(&penalty_items, Length::pt(25.0), TextAlignment::RaggedRight);
    let glue_lines = GreedyBreaker.break_lines(&glue_items, Length::pt(25.0), TextAlignment::RaggedRight);

    // Assert
    assert_eq!(penalty_lines.len(), glue_lines.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
    for (penalty_line, glue_line) in penalty_lines.iter().zip(&glue_lines) {
      assert_eq!(penalty_line.boxes.len(), glue_line.boxes.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
      for (penalty_box, glue_box) in penalty_line.boxes.iter().zip(&glue_line.boxes) {
        assert!(close_l(penalty_box.x, glue_box.x), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
      }
    }
  }
}
