//! 行分割

mod greedy;
mod knuth_plass;

use crate::{
  document::TextAlignment,
  length::{Length, LengthOverflow},
  typeset::boxes::{Align, FootnoteBody, HBox, HItem, IndexTerm, Line, LineLink, LinkTarget, PlacedHBox},
};

/// 水平リストを本文幅で行に分割する。両端揃えは段落全体最適（Knuth–Plass）、それ以外の揃えは貪欲法で組む。
/// 両端揃えでも、実現可能な分割が無いサブ段落は貪欲法の両端揃えで組む。両端揃えは行末の句読点を必要な場合だけ
/// `hang_room` の範囲でぶら下げる
///
/// `hang_room` は両端揃えの行末で句読点を利用幅の外へ出してよい幅。0 ならぶら下げない。両端揃え以外と貪欲法へ
/// 落ちたサブ段落は使わない。
///
/// # Errors
///
/// 行の幅や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
pub(super) fn break_lines(
  items: &[HItem],
  text_width: Length,
  alignment: TextAlignment,
  hang_room: Length,
) -> Result<Vec<Line>, LengthOverflow> {
  return if alignment == TextAlignment::Justify {
    knuth_plass::break_lines(items, text_width, hang_room)
  } else {
    greedy::break_lines(items, text_width, alignment, &mut Vec::new())
  };
}

/// 行分割中に開いている（`LinkStart` 済み・`LinkEnd` 未到達）リンク領域の状態
struct OpenLink {
  /// リンクの行き先
  target: LinkTarget,
  /// 現在の行における領域左端の行頭からの水平オフセット
  x0: Length,
}

/// 行の終わり方。語中ハイフンとぶら下げは同じ行に共存しない
#[derive(Clone, Copy)]
enum LineEnd<'a> {
  /// 行内アイテムだけで終わる
  Plain,
  /// 語中で折り返し、行内アイテムの直後にこのハイフンの箱を置く
  Hyphen(&'a HBox),
  /// 行末のこの句読点の箱を利用幅の外へぶら下げる（箱は行内アイテムに含まれる）
  Hang(&'a HBox),
}

/// 行頭の breakable glue を切り落としたスライスを返す
fn strip_leading_glue<'a, 'b>(items: &'b [&'a HItem]) -> &'b [&'a HItem] {
  let mut start = 0;
  while start < items.len()
    && matches!(
      items[start],
      HItem::Glue {
        breakable: true,
        ..
      }
    )
  {
    start += 1;
  }
  return &items[start..];
}

/// 行末の breakable glue を切り落としたスライスを返す（行末スペース不可視）
fn trim_trailing_glue<'a, 'b>(items: &'b [&'a HItem]) -> &'b [&'a HItem] {
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
  return &items[..end];
}

/// 行末の句読点の箱（ぶら下げの候補）を返す
///
/// 行末の breakable glue を除いた列の末尾から、幅 0 の運搬マーカー（`LinkStart` / `LinkEnd` / `IndexMark` /
/// `Footnote`）を読み飛ばした最後のアイテムが句読点（[`HItem::Comma`]）ならその箱、それ以外は `None`。
fn trailing_comma<'a>(items: &[&'a HItem]) -> Option<&'a HBox> {
  let last = trim_trailing_glue(items).iter().rev().copied().find(|item| {
    return !matches!(item, HItem::LinkStart(_) | HItem::LinkEnd | HItem::IndexMark(_) | HItem::Footnote(_));
  })?;
  return match last {
    HItem::Comma(hbox) => Some(hbox),
    _ => None,
  };
}

/// 行内アイテムの自然幅合計・伸長能力合計・収縮能力合計を求める
fn glue_metrics(items: &[&HItem]) -> Result<(Length, Length, Length), LengthOverflow> {
  let mut natural = Length::ZERO;
  let mut stretch = Length::ZERO;
  let mut shrink = Length::ZERO;
  for item in items {
    match item {
      HItem::FlushRight(_) => {},
      HItem::Glue {
        natural: n,
        stretch: s,
        shrink: sh,
        ..
      } => {
        natural = natural.try_add(*n)?;
        stretch = stretch.try_add(*s)?;
        shrink = shrink.try_add(*sh)?;
      },
      other => natural = natural.try_add(other.natural_width())?,
    }
  }
  return Ok((natural, stretch, shrink));
}

/// 行の余り幅から glue の伸縮係数を求める（両端揃えの配分係数）
fn glue_adjust_ratio(items: &[&HItem], available: Length) -> Result<f64, LengthOverflow> {
  let (natural, total_stretch, total_shrink) = glue_metrics(items)?;
  let leftover = available.try_sub(natural)?;
  if leftover > Length::ZERO {
    if !total_stretch.is_positive() {
      return Ok(0.0);
    }
    return Ok(leftover.ratio(total_stretch).min(1.0));
  }
  if leftover < Length::ZERO {
    if !total_shrink.is_positive() {
      return Ok(0.0);
    }
    return Ok(-((-leftover).ratio(total_shrink).min(1.0)));
  }
  return Ok(0.0);
}

/// アイテム列から 1 行を組み立てる（位置確定）
///
/// `alignment` の水平ずらしまで済ませる。右寄せ末尾ボックス（`HItem::FlushRight`）はずらさず利用可能幅の
/// 右端に置き、本文はその幅を除いた残りの中で寄せる。本文の幅は [`Line::width`] と同じ「箱の右端の最大」で測る
/// （行末の Kern 等の空白は寄せる幅に含めない）。`end` がハイフンなら行内アイテムの直後にハイフンを置き、
/// ぶら下げなら句読点を除いた本文が利用可能幅に揃うよう伸縮して [`Line::hanging`] に句読点の幅を入れる。
///
/// # Errors
///
/// 行内の位置や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
fn build_line(
  items: &[&HItem],
  is_last: bool,
  available: Length,
  alignment: TextAlignment,
  open_links: &mut Vec<OpenLink>,
  end: LineEnd<'_>,
) -> Result<Line, LengthOverflow> {
  let items = trim_trailing_glue(items);

  // 両端揃えの配分係数（正 = 伸長 / 負 = 収縮）。最終行は伸縮しない。
  // 行末ハイフンぶんを差し引き、ぶら下げる句読点ぶんを足した幅に対して配分する（ハイフン込み・句読点抜きで右端に揃える）
  let (trailing_hyphen, hanging) = match end {
    LineEnd::Plain => (None, Length::ZERO),
    LineEnd::Hyphen(hyphen) => (Some(hyphen), Length::ZERO),
    LineEnd::Hang(comma) => (None, comma.width),
  };
  let hyphen_width = trailing_hyphen.map_or(Length::ZERO, |hyphen| return hyphen.width);
  let adjust_ratio: f64 = if alignment == TextAlignment::Justify && !is_last {
    glue_adjust_ratio(items, available.try_sub(hyphen_width)?.try_add(hanging)?)?
  } else {
    0.0
  };

  // 前の行から継続するリンクは、この行では行頭（x0 = 0）から始まる
  for open in open_links.iter_mut() {
    open.x0 = Length::ZERO;
  }

  let mut boxes: Vec<PlacedHBox> = Vec::new();
  let mut links: Vec<LineLink> = Vec::new();
  let mut footnotes: Vec<FootnoteBody> = Vec::new();
  let mut index_marks: Vec<IndexTerm> = Vec::new();
  let mut flush_right: Vec<&HBox> = Vec::new();
  let mut x = Length::ZERO;
  let mut height = Length::ZERO;
  let mut depth = Length::ZERO;
  for item in items {
    match item {
      HItem::Box(hbox) | HItem::Comma(hbox) => {
        boxes.push(PlacedHBox {
          hbox: hbox.clone(),
          dx: x,
          dy: Length::ZERO,
        });
        x = x.try_add(hbox.width)?;
        height = height.max(hbox.height);
        depth = depth.max(hbox.depth);
      },
      HItem::Glue {
        natural,
        stretch,
        shrink,
        ..
      } => {
        let adjustment = if adjust_ratio >= 0.0 {
          stretch.try_scale(adjust_ratio)?
        } else {
          shrink.try_scale(adjust_ratio)?
        };
        x = x.try_add(natural.try_add(adjustment)?)?;
      },
      HItem::Kern(value) => x = x.try_add(*value)?,
      // 行内に残った数式内分割点は、折り返さなかったのでアキとして幅を持つ
      // （折り返した点のアイテムは破断アイテムとして行から除かれ、ここへは来ない）
      HItem::MathBreak { spacing, .. } => x = x.try_add(*spacing)?,
      // 右寄せ末尾ボックス: 行内累積 x を無視し、揃えのずらしの後で利用可能幅の右端へ置く
      HItem::FlushRight(hbox) => {
        flush_right.push(hbox);
        height = height.max(hbox.height);
        depth = depth.max(hbox.depth);
      },
      HItem::LinkStart(target) => open_links.push(OpenLink {
        target: target.clone(),
        x0: x,
      }),
      HItem::LinkEnd => {
        if let Some(open) = open_links.pop() {
          links.push(LineLink {
            target: open.target,
            x0: open.x0,
            x1: x,
          });
        }
      },
      // 脚注マーカーは幅 0・分割不可。この行の脚注として収集する
      HItem::Footnote(footnote) => footnotes.push(footnote.clone()),
      // 索引マーカーは幅 0・分割不可。この行の索引語として収集する
      HItem::IndexMark(term) => index_marks.push(term.clone()),
      // 行内の Discretionary は描画しない（折り返し位置のハイフンは `LineEnd::Hyphen` で出す）
      HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak => {},
    }
  }
  // 語中で折り返した行は、行内アイテムの直後（両端揃えでは伸縮後の右端）にハイフンを置く
  if let Some(hyphen) = trailing_hyphen {
    boxes.push(PlacedHBox {
      hbox: hyphen.clone(),
      dx: x,
      dy: Length::ZERO,
    });
    x = x.try_add(hyphen.width)?;
    height = height.max(hyphen.height);
    depth = depth.max(hyphen.depth);
  }
  // 行末でまだ開いているリンクは、この行ぶんの矩形を出して次行へ継続する
  for open in open_links.iter() {
    links.push(LineLink {
      target: open.target.clone(),
      x0: open.x0,
      x1: x,
    });
  }
  let mut line = Line {
    boxes,
    height,
    depth,
    links,
    footnotes,
    index_marks,
    hanging,
  };
  let flush_width = flush_right.iter().map(|hbox| return hbox.width).fold(Length::ZERO, Length::max);
  let body_width = line.width_without_hanging()?;
  line.shift_x(Align::from(alignment).offset(available.try_sub(flush_width)?, body_width)?)?;
  for hbox in flush_right {
    line.boxes.push(PlacedHBox {
      hbox: hbox.clone(),
      dx: available.try_sub(hbox.width)?.max(Length::ZERO),
      dy: Length::ZERO,
    });
  }
  return Ok(line);
}

/// 行分割が改行できる位置の間で最も広い区間の幅（セルの最小内容幅）
///
/// 分割機会は breakable な `Glue`（幅は区間に入れない）・`Penalty { value <= 0 }`・`Discretionary`（直前の区間に
/// ハイフン幅を足す）・`MathBreak`（`spacing` は区間に入れない）・`ForcedBreak`。それ以外は自然幅を区間へ足し、
/// non-breakable な `Glue` と正の `Penalty` は区間を切らない。ハイフネーションは greedy が持ち越し後に選び直さない
/// ので真の下限にはならない（その行は呼び出し側の overflow 警告が受ける）。
///
/// # Errors
///
/// 区間の幅の和が長さの値域を超えたら [`LengthOverflow`] を返す。
pub(super) fn min_content_width(items: &[HItem]) -> Result<Length, LengthOverflow> {
  let mut widest = Length::ZERO;
  let mut segment = Length::ZERO;
  for item in items {
    match item {
      HItem::Box(_) | HItem::Comma(_) | HItem::FlushRight(_) | HItem::Kern(_) => {
        segment = segment.try_add(item.natural_width())?;
      },
      HItem::Glue {
        natural, breakable, ..
      } => {
        if *breakable {
          widest = widest.max(segment);
          segment = Length::ZERO;
        } else {
          segment = segment.try_add(*natural)?;
        }
      },
      HItem::Penalty { value } => {
        if *value <= 0 {
          widest = widest.max(segment);
          segment = Length::ZERO;
        }
      },
      HItem::Discretionary { hyphen } => {
        widest = widest.max(segment.try_add(hyphen.width)?);
        segment = Length::ZERO;
      },
      HItem::MathBreak { .. } | HItem::ForcedBreak => {
        widest = widest.max(segment);
        segment = Length::ZERO;
      },
      HItem::LinkStart(_) | HItem::LinkEnd | HItem::Footnote(_) | HItem::IndexMark(_) => {},
    }
  }
  return Ok(widest.max(segment));
}

/// 行分割テストの共有フィクスチャ
#[cfg(test)]
pub(super) mod test_support {
  use crate::{
    length::Length,
    semantics::LabelId,
    typeset::boxes::{AnchorId, HBox, HBoxContent, HItem, IndexTerm, LinkTarget},
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// テスト用の合成ボックス（幅 10、高さ 8、深さ 2）
  pub(in crate::typeset::boxing) fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(10.0),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の指定幅ボックス（高さ 8・深さ 2）
  pub(in crate::typeset::boxing) fn box_width(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の breakable glue（幅 5・伸縮なし）
  pub(in crate::typeset::boxing) fn space_glue() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: Length::ZERO,
      shrink: Length::ZERO,
      breakable: true,
    };
  }

  /// テスト用の右寄せ末尾ボックス（QED 相当・指定幅、高さ 6・深さ 0）
  pub(super) fn flush_right_box(width: f32) -> HItem {
    return HItem::FlushRight(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(6.0),
      depth: Length::ZERO,
    });
  }

  /// テスト用の伸縮能力付き breakable glue（幅 5・伸長 2.5・収縮 5/3 = 単語間スペース相当）
  pub(in crate::typeset::boxing) fn stretch_glue() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: pt(2.5),
      shrink: pt(5.0 / 3.0),
      breakable: true,
    };
  }

  /// テスト用の伸縮能力付き非 breakable glue（分割点にならず overflow 行を作れる）
  pub(super) fn non_breakable_stretch_glue() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: pt(2.5),
      shrink: pt(5.0 / 3.0),
      breakable: false,
    };
  }

  /// テスト用の和文字間 glue（幅 0・伸長 0.5・収縮なし = フォントサイズ 10pt の字間相当）
  pub(super) fn cjk_glue() -> HItem {
    return HItem::Glue {
      natural: Length::ZERO,
      stretch: pt(0.5),
      shrink: Length::ZERO,
      breakable: true,
    };
  }

  /// テスト用の和文句読点（指定幅・高さ 8・深さ 2）
  pub(super) fn comma(width: f32) -> HItem {
    return HItem::Comma(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の句読点の後ろの約物アキ（幅 5・伸長なし・収縮 5・分割可）
  pub(super) fn comma_aki() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: Length::ZERO,
      shrink: pt(5.0),
      breakable: true,
    };
  }

  /// ぶら下げの基本列: 箱 10 ×3（和文字間 glue で区切る）・句読点 5・約物アキ・箱 10（自然幅 35 で句読点だけが
  /// 幅 30 を溢れる）
  pub(super) fn hang_items() -> Vec<HItem> {
    return vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      comma_aki(),
      box_width(10.0),
    ];
  }

  /// テスト用の語中ハイフネーション分割点（指定幅のハイフン箱・高さ 6・深さ 0）
  pub(super) fn discretionary(hyphen_width: f32) -> HItem {
    return HItem::Discretionary {
      hyphen: HBox {
        content: HBoxContent::Atom(Vec::new()),
        width: pt(hyphen_width),
        height: pt(6.0),
        depth: Length::ZERO,
      },
    };
  }

  /// テスト用の数式内分割点（折り返さないときのアキ・ペナルティを指定）
  pub(super) fn math_break(spacing: f32, penalty: i32) -> HItem {
    return HItem::MathBreak {
      spacing: pt(spacing),
      penalty,
    };
  }

  /// テスト用の内部リンク行き先
  pub(super) fn link_target() -> LinkTarget { return LinkTarget::Internal(AnchorId::Label(LabelId::new("sec:x"))); }

  /// テスト用の索引マーカー（幅 0・分割不可）
  pub(super) fn index_mark(word: &str, reading: Option<&str>) -> HItem {
    return HItem::IndexMark(IndexTerm {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    });
  }
}

#[cfg(test)]
mod tests {
  use super::{HItem, Length, Line, TextAlignment, break_lines, greedy, knuth_plass, min_content_width};
  use crate::typeset::boxing::break_lines::test_support::{
    box_width, discretionary, hang_items, math_break, non_breakable_stretch_glue, space_glue, stretch_glue, test_box,
  };

  /// 2 つの行列が同じ組み方か（行数・各行の box 数・各 box の `dx` が sp 丸め精度内で一致）を確かめる
  fn assert_same_lines(actual: &[Line], expected: &[Line]) {
    assert_eq!(actual.len(), expected.len(), "actual: {actual:?}, expected: {expected:?}");
    for (actual_line, expected_line) in actual.iter().zip(expected) {
      assert_eq!(actual_line.boxes.len(), expected_line.boxes.len(), "actual: {actual:?}, expected: {expected:?}");
      for (a, e) in actual_line.boxes.iter().zip(&expected_line.boxes) {
        assert!((a.dx - e.dx).abs() <= Length::from_sp(1), "actual: {actual:?}, expected: {expected:?}");
      }
    }
  }

  #[test]
  fn non_justified_alignments_break_greedily() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    for alignment in [
      TextAlignment::Left,
      TextAlignment::Center,
      TextAlignment::Right,
    ] {
      let lines = break_lines(&items, Length::pt(27.0), alignment, Length::ZERO).expect("テストの寸法は値域内");
      let greedy =
        greedy::break_lines(&items, Length::pt(27.0), alignment, &mut Vec::new()).expect("テストの寸法は値域内");

      assert_same_lines(&lines, &greedy);
    }
  }

  #[test]
  fn justified_alignment_breaks_with_knuth_plass() {
    let glue = || {
      return HItem::Glue {
        natural: Length::pt(2.0),
        stretch: Length::pt(1.0),
        shrink: Length::pt(2.0),
        breakable: true,
      };
    };
    let items = vec![
      box_width(8.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
    ];

    let lines =
      break_lines(&items, Length::pt(25.0), TextAlignment::Justify, Length::ZERO).expect("テストの寸法は値域内");
    let optimal = knuth_plass::break_lines(&items, Length::pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_same_lines(&lines, &optimal);
    // 貪欲法なら 3 行になる入力
    assert_eq!(lines.len(), 2, "{lines:?}");
  }

  #[test]
  fn non_justified_alignments_never_hang() {
    for alignment in [
      TextAlignment::Left,
      TextAlignment::Center,
      TextAlignment::Right,
    ] {
      let lines =
        break_lines(&hang_items(), Length::pt(30.0), alignment, Length::pt(5.0)).expect("テストの寸法は値域内");

      assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{alignment:?}: {lines:?}");
    }
  }

  #[test]
  fn min_content_width_is_the_widest_segment_between_glue() {
    let items = vec![
      box_width(10.0),
      space_glue(),
      box_width(30.0),
      space_glue(),
      box_width(20.0),
    ];

    assert_eq!(min_content_width(&items).expect("テストの寸法は値域内"), Length::pt(30.0));
  }

  #[test]
  fn min_content_width_adds_the_hyphen_to_the_segment_before_a_discretionary() {
    // 12 + ハイフン 3 = 15 が 10 より広い
    let items = [box_width(12.0), discretionary(3.0), box_width(10.0)];

    assert_eq!(min_content_width(&items).expect("テストの寸法は値域内"), Length::pt(15.0));
  }

  #[test]
  fn min_content_width_breaks_at_math_breaks_without_their_spacing() {
    let items = [box_width(10.0), math_break(4.0, 0), box_width(10.0)];

    assert_eq!(min_content_width(&items).expect("テストの寸法は値域内"), Length::pt(10.0));
  }

  #[test]
  fn min_content_width_keeps_non_breakable_glue_and_positive_penalty_inside_a_segment() {
    let items = vec![
      box_width(10.0),
      non_breakable_stretch_glue(),
      box_width(10.0),
      HItem::Penalty { value: 100 },
      box_width(5.0),
    ];

    assert_eq!(min_content_width(&items).expect("テストの寸法は値域内"), Length::pt(30.0));
  }

  #[test]
  fn min_content_width_of_no_items_is_zero() {
    assert_eq!(min_content_width(&[]).expect("テストの寸法は値域内"), Length::ZERO);
  }

  #[test]
  fn greedy_never_overflows_at_the_min_content_width() {
    // 分割機会が glue / penalty / 数式内分割点だけの列で、最小内容幅で折ると全行が収まる（定義が行分割と一致する）
    let items = vec![
      box_width(7.0),
      space_glue(),
      box_width(18.0),
      HItem::Penalty { value: 0 },
      box_width(4.0),
      math_break(2.0, 0),
      box_width(11.0),
      space_glue(),
      box_width(3.0),
    ];
    let width = min_content_width(&items).expect("テストの寸法は値域内");

    let lines = break_lines(&items, width, TextAlignment::Left, Length::ZERO).expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.width().expect("値域内") <= width), "{lines:?}");
  }
}
