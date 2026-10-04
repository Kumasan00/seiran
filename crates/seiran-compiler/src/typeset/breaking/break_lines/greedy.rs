//! 貪欲法（first-fit）による行分割

use tracing::trace;

use crate::{
  document::TextAlignment,
  length::Length,
  typeset::{
    boxes::{HItem, Line},
    breaking::break_lines::{LineBreaker, OpenLink, build_line},
    observe,
  },
};

/// 貪欲法（first-fit）による行分割
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::typeset::breaking) struct GreedyBreaker;

impl LineBreaker for GreedyBreaker {
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut buffer: Vec<&HItem> = Vec::new();
    let mut width_so_far = Length::ZERO;
    // 通常の分割点（Glue / Penalty / Discretionary）の最新位置
    let mut last_break: Option<usize> = None;
    // 数式内分割点（`HItem::MathBreak`）の最新位置。通常の分割点が無いときだけ使う退避先
    let mut last_math_break: Option<usize> = None;
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
          last_math_break = None;
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
        HItem::MathBreak { spacing, .. } => {
          buffer.push(item);
          last_math_break = Some(buffer.len() - 1);
          // 折り返さなければアキとして幅を持つ
          width_so_far += *spacing;
        },
        // リンクマーカー・脚注マーカー・索引マーカーは幅 0・分割不可
        HItem::LinkStart(_) | HItem::LinkEnd | HItem::Footnote(_) | HItem::IndexMark(_) => {
          buffer.push(item);
        },
        HItem::Box(_) | HItem::Kern(_) | HItem::FlushRight(_) => {
          let item_width = item.natural_width();
          // 1 回の持ち越しで収まるとは限らない（持ち越し後も現在の item を含めて溢れることがある）ので
          // while で再判定する。持ち越すたびに buffer は真に縮む（少なくとも破断アイテム自身が抜ける）ので
          // いずれ last_break / last_math_break が尽きて（あるいは収まって）終わる
          while width_so_far + item_width > text_width
            && let Some(break_index) = last_break.or(last_math_break)
          {
            let trailing_hyphen = match buffer[break_index] {
              HItem::Discretionary { hyphen } => Some(hyphen),
              _ => None,
            };
            let line =
              build_line(&buffer[..break_index], false, text_width, alignment, &mut open_links, trailing_hyphen);
            push_line(&mut lines, line, false, trailing_hyphen.is_some());
            let carried: Vec<&HItem> = buffer[break_index + 1..].to_vec();
            buffer = carried;
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
            // 通常の分割点で折ったとき、それより後ろにあった数式内分割点は持ち越し側に残る
            // （通常の分割点は選んだ点より後ろに無い）ので、持ち越し後の buffer から拾い直す
            last_math_break = buffer.iter().rposition(|carried| return matches!(carried, HItem::MathBreak { .. }));
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
/// `line_index` は段落内の連番（0 起点。強制改行でもリセットしない）。
fn push_line(lines: &mut Vec<Line>, line: Line, is_last: bool, hyphen: bool) {
  trace!(
    line_index = lines.len(),
    is_last,
    is_hyphenated = hyphen,
    width_pt = %line.width().to_pt(),
    text = observe::summarize_line(&line),
    "貪欲法で行を確定"
  );
  lines.push(line);
}

#[cfg(test)]
mod tests {
  use super::{GreedyBreaker, LineBreaker};
  use crate::{
    document::TextAlignment,
    length::Length,
    typeset::{
      boxes::HItem,
      breaking::break_lines::test_support::{
        box_width, cjk_glue, discretionary, flush_right_box, index_mark, link_target, math_break,
        non_breakable_stretch_glue, space_glue, stretch_glue, test_box,
      },
    },
  };

  /// `Length` が pt 値 `expected` に（差 1e-3 pt 未満で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 2 つの `Length` が（sp 丸め精度内で）一致するか
  fn close_l(a: Length, b: Length) -> bool { return (a - b).abs() <= Length::from_sp(1); }

  #[test]
  fn breaks_at_glue_when_box_exceeds_width() {
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2);
    assert_eq!(lines[1].boxes.len(), 1);
    assert!(close(lines[1].boxes[0].dx, 0.0));
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].boxes.len(), 2);
    assert!(close(lines[0].height, 8.0));
    assert!(close(lines[0].depth, 2.0));
    assert!(lines[0].links.is_empty());
  }

  #[test]
  fn index_mark_is_collected_without_affecting_width_or_breaks() {
    let items = vec![test_box(), index_mark("語", None), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].boxes.len(), 2, "index_mark はボックスとして描画されない");
    assert!(close(lines[0].boxes[1].dx, 10.0), "index_mark を挟んでも 2 つ目の box の x は不変: {lines:?}");
    assert_eq!(lines[0].index_marks.len(), 1);
    assert_eq!(lines[0].index_marks[0].word, "語");
    assert_eq!(lines[0].index_marks[0].reading, None);
  }

  #[test]
  fn breaks_at_zero_penalty_between_boxes() {
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Left);

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

    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1, "分割点がなければ overflow 許容: {lines:?}");
    assert_eq!(lines[0].boxes.len(), 3);
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    let lines = GreedyBreaker.break_lines(&[], Length::pt(100.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    let items = vec![test_box(), space_glue(), flush_right_box(8.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(50.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED box の 2 つ: {lines:?}");
    assert!(close(lines[0].boxes[0].dx, 0.0));
    assert!(close(lines[0].boxes[1].dx, 42.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn flush_right_box_wraps_to_next_line_when_it_does_not_fit() {
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(14.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert!(close(lines[0].boxes[0].dx, 0.0));
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED box のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].dx, 6.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn link_spanning_wrap_splits_into_two_rects() {
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      space_glue(),
      test_box(),
      HItem::LinkEnd,
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(12.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "1 行目に継続中の矩形: {:?}", lines[0].links);
    assert!(close(lines[0].links[0].x1, 10.0));
    assert_eq!(lines[1].links.len(), 1, "2 行目に残りの矩形: {:?}", lines[1].links);
    assert!(close(lines[1].links[0].x0, 0.0));
    assert!(close(lines[1].links[0].x1, 10.0));
  }

  #[test]
  fn justify_stretches_glue_to_flush_right_edge() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 17.0), "{lines:?}");
    let right_edge = lines[0].boxes[1].dx + lines[0].boxes[1].hbox.width;
    assert!(close(right_edge, 27.0), "非最終行の右端は版面右端に一致: {lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_last_line() {
    let items = vec![test_box(), stretch_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1);
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_line_before_forced_break() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_stretch_limit() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 17.5), "伸長は能力の上限で止まる: {lines:?}");
  }

  #[test]
  fn justify_shrinks_overfull_line() {
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(24.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 14.0), "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_shrink_limit() {
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(23.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0 - 5.0 / 3.0), "収縮は能力の下限で止まる: {lines:?}");
  }

  #[test]
  fn justify_leaves_line_without_stretch_points_ragged() {
    let items = vec![
      test_box(),
      HItem::Kern(Length::pt(5.0)),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(26.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_moves_link_rects_with_stretched_glue() {
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::LinkEnd,
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!(close(lines[0].links[0].x0, 0.0));
    assert!(close(lines[0].links[0].x1, 27.0), "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn left_ignores_stretch_capacity() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    let items = vec![
      test_box(),
      discretionary(3.0),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 3, "本文 box 2 つ + 行末ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[2].dx, 20.0), "{lines:?}");
    assert!(close(lines[0].boxes[2].hbox.width, 3.0), "{lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| return b.dx + b.hbox.width).fold(Length::ZERO, Length::max).to_pt();
    assert!(right_edge <= 25.0 + f32::EPSILON, "ハイフン込みで右端超過なし: {right_edge}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn discretionary_not_used_when_word_fits() {
    let items = vec![test_box(), discretionary(3.0), test_box()];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Left);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "ハイフン箱は付かない: {lines:?}");
  }

  #[test]
  fn discretionary_rejected_when_hyphen_would_overflow() {
    let items = vec![
      test_box(),
      discretionary(1.0),
      test_box(),
      discretionary(20.0),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(22.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "box1 + ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[1].hbox.width, 1.0), "使われたのは disc1 のハイフン: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
  }

  #[test]
  fn justify_includes_hyphen_width_at_flush_right_edge() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(29.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 16.0), "glue 伸長後の box2: {lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| return b.dx + b.hbox.width).fold(Length::ZERO, Length::max);
    assert!(close(right_edge, 29.0), "ハイフン込みで右端に揃う: {}", right_edge.to_pt());
  }

  #[test]
  fn cjk_zero_width_glue_breaks_like_zero_penalty() {
    let penalty_items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];
    let glue_items = vec![test_box(), cjk_glue(), test_box(), cjk_glue(), test_box()];

    let penalty_lines = GreedyBreaker.break_lines(&penalty_items, Length::pt(25.0), TextAlignment::Left);
    let glue_lines = GreedyBreaker.break_lines(&glue_items, Length::pt(25.0), TextAlignment::Left);

    assert_eq!(penalty_lines.len(), glue_lines.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
    for (penalty_line, glue_line) in penalty_lines.iter().zip(&glue_lines) {
      assert_eq!(penalty_line.boxes.len(), glue_line.boxes.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
      for (penalty_box, glue_box) in penalty_line.boxes.iter().zip(&glue_line.boxes) {
        assert!(close_l(penalty_box.dx, glue_box.dx), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
      }
    }
  }

  #[test]
  fn breaks_at_math_break_when_no_other_breakpoint() {
    let items = vec![box_width(20.0), math_break(3.0, 500), box_width(20.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "{lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "{lines:?}");
    assert_eq!(lines[1].boxes[0].dx, Length::ZERO, "演算子後のアキは次行の行頭に残らない: {lines:?}");
  }

  #[test]
  fn prefers_ordinary_break_over_math_break() {
    let items = vec![
      box_width(10.0),
      space_glue(),
      box_width(10.0),
      math_break(3.0, 500),
      box_width(10.0),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Left);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "空白で折る: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
    assert_eq!(lines[1].boxes[1].dx, Length::pt(13.0), "折らなかった分割点はアキとして残る: {lines:?}");
  }

  #[test]
  fn math_break_carried_past_ordinary_break_stays_usable() {
    let items = vec![
      box_width(5.0),
      space_glue(),
      box_width(5.0),
      math_break(3.0, 500),
      box_width(10.0),
      box_width(10.0),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(20.0), TextAlignment::Left);

    let box_counts: Vec<usize> = lines.iter().map(|line| return line.boxes.len()).collect();
    assert_eq!(box_counts, vec![1, 1, 2], "{lines:?}");
  }

  #[test]
  fn carried_math_break_is_used_for_the_item_that_overflowed() {
    // glue で折った後の持ち越し [b10, MB] はそれ単体では 20 に収まるが、次の b10 を足すと再び溢れる
    let items = vec![
      box_width(5.0),
      space_glue(),
      box_width(10.0),
      math_break(3.0, 500),
      box_width(10.0),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(20.0), TextAlignment::Left);

    let box_counts: Vec<usize> = lines.iter().map(|line| return line.boxes.len()).collect();
    assert_eq!(box_counts, vec![1, 1, 1], "{lines:?}");
  }

  #[test]
  fn center_puts_line_in_middle_of_available_width() {
    let items = vec![box_width(20.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Center);

    assert!(close(lines[0].boxes[0].dx, 40.0), "{:?}", lines[0].boxes);
  }

  #[test]
  fn right_shifts_each_line_to_right_edge_independently() {
    // 幅 27: [箱 10・アキ 5・箱 10]（自然幅 25）/ [箱 10] の 2 行に割れる
    let items = vec![
      test_box(),
      space_glue(),
      test_box(),
      space_glue(),
      test_box(),
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Right);

    assert_eq!(lines.len(), 2);
    assert!(close(lines[0].boxes[0].dx, 2.0), "{:?}", lines[0].boxes);
    assert!(close(lines[1].boxes[0].dx, 17.0), "{:?}", lines[1].boxes);
  }

  #[test]
  fn center_and_right_do_not_stretch_glue() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    for alignment in [TextAlignment::Center, TextAlignment::Right] {
      let lines = GreedyBreaker.break_lines(&items, Length::pt(27.0), alignment);

      let first = &lines[0];
      assert!(close(first.boxes[1].dx - first.boxes[0].dx, 15.0), "{alignment:?}: {:?}", first.boxes);
    }
  }

  #[test]
  fn overflowing_line_is_not_shifted_left_of_origin() {
    let items = vec![box_width(50.0)];

    for alignment in [TextAlignment::Center, TextAlignment::Right] {
      let lines = GreedyBreaker.break_lines(&items, Length::pt(30.0), alignment);

      assert_eq!(lines[0].boxes[0].dx, Length::ZERO, "{alignment:?}");
    }
  }

  #[test]
  fn leading_kern_is_shifted_with_line_content() {
    // 字下げ 10 + 本文 20 = 30 を幅 100 の右端へ: 本文の箱は 70 + 10 = 80
    let items = vec![HItem::Kern(Length::pt(10.0)), box_width(20.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Right);

    assert!(close(lines[0].boxes[0].dx, 80.0), "{:?}", lines[0].boxes);
  }

  #[test]
  fn centered_line_with_qed_centers_body_left_of_mark() {
    // 本文 20 + QED 10、幅 100: 本文は 0..90 の中央（35）、QED は右端（90）
    let items = vec![box_width(20.0), flush_right_box(10.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Center);

    assert_eq!(lines.len(), 1);
    assert!(close(lines[0].boxes[0].dx, 35.0), "{:?}", lines[0].boxes);
    assert!(close(lines[0].boxes[1].dx, 90.0), "{:?}", lines[0].boxes);
  }

  #[test]
  fn right_aligned_line_with_qed_ends_body_at_mark() {
    let items = vec![box_width(20.0), flush_right_box(10.0)];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Right);

    assert!(close(lines[0].boxes[0].dx, 70.0), "{:?}", lines[0].boxes);
    assert!(close(lines[0].boxes[1].dx, 90.0), "{:?}", lines[0].boxes);
  }

  #[test]
  fn link_rectangle_moves_with_centered_line() {
    let items = vec![
      HItem::LinkStart(link_target()),
      box_width(20.0),
      HItem::LinkEnd,
    ];

    let lines = GreedyBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Center);

    let link = &lines[0].links[0];
    assert!(close(link.x0, 40.0) && close(link.x1, 60.0), "{link:?}");
  }
}
