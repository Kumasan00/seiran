//! (c) 行分割

mod greedy;
mod knuth_plass;

pub(super) use greedy::GreedyBreaker;
pub(in crate::typeset) use knuth_plass::KnuthPlassBreaker;

use crate::{
  document::TextAlignment,
  length::Length,
  typeset::boxes::{Align, HBox, HItem, IndexTerm, Line, LineLink, LinkTarget, MeasuredFootnote, PlacedHBox},
};

/// 行分割アルゴリズムの抽象
pub(in crate::typeset) trait LineBreaker {
  /// 水平リストを本文幅で行に分割する
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line>;
}

/// 行分割中に開いている（`LinkStart` 済み・`LinkEnd` 未到達）リンク領域の状態
struct OpenLink {
  /// リンクの行き先
  target: LinkTarget,
  /// 現在の行における領域左端の行頭からの水平オフセット
  x0: Length,
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

/// 行内アイテムの自然幅合計・伸長能力合計・収縮能力合計を求める
fn glue_metrics(items: &[&HItem]) -> (Length, Length, Length) {
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
        natural += *n;
        stretch += *s;
        shrink += *sh;
      },
      other => natural += other.natural_width(),
    }
  }
  return (natural, stretch, shrink);
}

/// 行の余り幅から glue の伸縮係数を求める（両端揃えの配分係数）
fn glue_adjust_ratio(items: &[&HItem], available: Length) -> f64 {
  let (natural, total_stretch, total_shrink) = glue_metrics(items);
  let leftover = available - natural;
  if leftover > Length::ZERO {
    if !total_stretch.is_positive() {
      return 0.0;
    }
    return leftover.ratio(total_stretch).min(1.0);
  }
  if leftover < Length::ZERO {
    if !total_shrink.is_positive() {
      return 0.0;
    }
    return -((-leftover).ratio(total_shrink).min(1.0));
  }
  return 0.0;
}

/// アイテム列から 1 行を組み立てる（位置確定）
///
/// `alignment` の水平ずらしまで済ませる。右寄せ末尾ボックス（`HItem::FlushRight`）はずらさず利用可能幅の
/// 右端に置き、本文はその幅を除いた残りの中で寄せる。本文の幅は [`Line::width`] と同じ「箱の右端の最大」で測る
/// （行末の Kern 等の空白は寄せる幅に含めない）。
fn build_line(
  items: &[&HItem],
  is_last: bool,
  available: Length,
  alignment: TextAlignment,
  open_links: &mut Vec<OpenLink>,
  trailing_hyphen: Option<&HBox>,
) -> Line {
  let items = trim_trailing_glue(items);

  // 両端揃えの配分係数（正 = 伸長 / 負 = 収縮）。最終行は伸縮しない。
  // 行末ハイフンぶんを本文幅から差し引いた余り幅で配分する（ハイフン込みで右端に揃える）
  let hyphen_width = trailing_hyphen.map_or(Length::ZERO, |hyphen| return hyphen.width);
  let adjust_ratio: f64 = if alignment == TextAlignment::Justify && !is_last {
    glue_adjust_ratio(items, available - hyphen_width)
  } else {
    0.0
  };

  // 前の行から継続するリンクは、この行では行頭（x0 = 0）から始まる
  for open in open_links.iter_mut() {
    open.x0 = Length::ZERO;
  }

  let mut boxes: Vec<PlacedHBox> = Vec::new();
  let mut links: Vec<LineLink> = Vec::new();
  let mut footnotes: Vec<MeasuredFootnote> = Vec::new();
  let mut index_marks: Vec<IndexTerm> = Vec::new();
  let mut flush_right: Vec<&HBox> = Vec::new();
  let mut x = Length::ZERO;
  let mut height = Length::ZERO;
  let mut depth = Length::ZERO;
  for item in items {
    match item {
      HItem::Box(hbox) => {
        boxes.push(PlacedHBox {
          hbox: hbox.clone(),
          dx: x,
          dy: Length::ZERO,
        });
        x += hbox.width;
        height = height.max(hbox.height);
        depth = depth.max(hbox.depth);
      },
      HItem::Glue {
        natural,
        stretch,
        shrink,
        ..
      } => {
        x += *natural
          + if adjust_ratio >= 0.0 {
            stretch.scale(adjust_ratio)
          } else {
            shrink.scale(adjust_ratio)
          };
      },
      HItem::Kern(value) => x += *value,
      // 行内に残った数式内分割点は、折り返さなかったのでアキとして幅を持つ
      // （折り返した点のアイテムは破断アイテムとして行から除かれ、ここへは来ない）
      HItem::MathBreak { spacing, .. } => x += *spacing,
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
      // 行内の Discretionary は描画しない（折り返し位置のハイフンは trailing_hyphen で出す）
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
    x += hyphen.width;
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
  };
  let flush_width = flush_right.iter().map(|hbox| return hbox.width).fold(Length::ZERO, Length::max);
  let body_width = line.width();
  line.shift_x(Align::from(alignment).offset(available - flush_width, body_width));
  for hbox in flush_right {
    line.boxes.push(PlacedHBox {
      hbox: hbox.clone(),
      dx: (available - hbox.width).max(Length::ZERO),
      dy: Length::ZERO,
    });
  }
  return line;
}

/// 行分割器が改行できる位置の間で最も広い区間の幅（セルの最小内容幅）
///
/// 分割機会は breakable な `Glue`（幅は区間に入れない）・`Penalty { value <= 0 }`・`Discretionary`（直前の区間に
/// ハイフン幅を足す）・`MathBreak`（`spacing` は区間に入れない）・`ForcedBreak`。それ以外は自然幅を区間へ足し、
/// non-breakable な `Glue` と正の `Penalty` は区間を切らない。ハイフネーションは greedy が持ち越し後に選び直さない
/// ので真の下限にはならない（その行は呼び出し側の overflow 警告が受ける）。
pub(in crate::typeset) fn min_content_width(items: &[HItem]) -> Length {
  let mut widest = Length::ZERO;
  let mut segment = Length::ZERO;
  for item in items {
    match item {
      HItem::Box(_) | HItem::FlushRight(_) | HItem::Kern(_) => segment += item.natural_width(),
      HItem::Glue {
        natural, breakable, ..
      } => {
        if *breakable {
          widest = widest.max(segment);
          segment = Length::ZERO;
        } else {
          segment += *natural;
        }
      },
      HItem::Penalty { value } => {
        if *value <= 0 {
          widest = widest.max(segment);
          segment = Length::ZERO;
        }
      },
      HItem::Discretionary { hyphen } => {
        widest = widest.max(segment + hyphen.width);
        segment = Length::ZERO;
      },
      HItem::MathBreak { .. } | HItem::ForcedBreak => {
        widest = widest.max(segment);
        segment = Length::ZERO;
      },
      HItem::LinkStart(_) | HItem::LinkEnd | HItem::Footnote(_) | HItem::IndexMark(_) => {},
    }
  }
  return widest.max(segment);
}

/// 行分割テストの共有フィクスチャ
#[cfg(test)]
mod test_support {
  use crate::{
    length::Length,
    semantics::LabelId,
    typeset::boxes::{AnchorId, HBox, HBoxContent, HItem, IndexTerm, LinkTarget},
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// テスト用の合成ボックス（幅 10、高さ 8、深さ 2）
  pub(super) fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(10.0),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の指定幅ボックス（高さ 8・深さ 2）
  pub(super) fn box_width(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の breakable glue（幅 5・伸縮なし）
  pub(super) fn space_glue() -> HItem {
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
  pub(super) fn stretch_glue() -> HItem {
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
  use super::{GreedyBreaker, HItem, Length, LineBreaker, TextAlignment, min_content_width};
  use crate::typeset::breaking::break_lines::test_support::{
    box_width, discretionary, math_break, non_breakable_stretch_glue, space_glue,
  };

  #[test]
  fn min_content_width_is_the_widest_segment_between_glue() {
    let items = vec![
      box_width(10.0),
      space_glue(),
      box_width(30.0),
      space_glue(),
      box_width(20.0),
    ];

    assert_eq!(min_content_width(&items), Length::pt(30.0));
  }

  #[test]
  fn min_content_width_adds_the_hyphen_to_the_segment_before_a_discretionary() {
    // 12 + ハイフン 3 = 15 が 10 より広い
    let items = [box_width(12.0), discretionary(3.0), box_width(10.0)];

    assert_eq!(min_content_width(&items), Length::pt(15.0));
  }

  #[test]
  fn min_content_width_breaks_at_math_breaks_without_their_spacing() {
    let items = [box_width(10.0), math_break(4.0, 0), box_width(10.0)];

    assert_eq!(min_content_width(&items), Length::pt(10.0));
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

    assert_eq!(min_content_width(&items), Length::pt(30.0));
  }

  #[test]
  fn min_content_width_of_no_items_is_zero() {
    assert_eq!(min_content_width(&[]), Length::ZERO);
  }

  #[test]
  fn greedy_never_overflows_at_the_min_content_width() {
    // 分割機会が glue / penalty / 数式内分割点だけの列で、最小内容幅で折ると全行が収まる（定義が行分割器と一致する）
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
    let width = min_content_width(&items);

    let lines = GreedyBreaker.break_lines(&items, width, TextAlignment::Left);

    assert!(lines.iter().all(|line| return line.width() <= width), "{lines:?}");
  }
}
