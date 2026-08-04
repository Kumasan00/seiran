//! (c) 行分割

use model::{Length, LinkTarget, TextAlignment};

use crate::typeset::layout::{HBox, HItem, Line, LineFootnote, LineIndexEntry, LineLink, PositionedBox};

mod greedy;
mod knuth_plass;

pub use greedy::GreedyBreaker;
pub use knuth_plass::KnuthPlassBreaker;

/// 行分割アルゴリズムの抽象
pub trait LineBreaker {
  /// 水平リストを本文幅で行に分割する
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line>;
}

/// 行分割中に開いている（`LinkStart` 済み・`LinkEnd` 未到達）リンク領域の状態
pub(super) struct OpenLink {
  /// リンクの行き先
  target: LinkTarget,
  /// 現在の行における領域左端の行頭からの水平オフセット
  x0: Length,
}

/// 行頭の breakable glue を切り落としたスライスを返す
pub(super) fn strip_leading_glue<'a, 'b>(items: &'b [&'a HItem]) -> &'b [&'a HItem] {
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
pub(super) fn trim_trailing_glue<'a, 'b>(items: &'b [&'a HItem]) -> &'b [&'a HItem] {
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
pub(super) fn glue_metrics(items: &[&HItem]) -> (Length, Length, Length) {
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
pub(super) fn glue_adjust_ratio(items: &[&HItem], available: Length) -> f64 {
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
pub(super) fn build_line(
  items: &[&HItem],
  is_last: bool,
  available: Length,
  alignment: TextAlignment,
  open_links: &mut Vec<OpenLink>,
  trailing_hyphen: Option<&HBox>,
) -> Line {
  // 行末の breakable glue を切り落とす
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

  let mut boxes: Vec<PositionedBox> = Vec::new();
  let mut links: Vec<LineLink> = Vec::new();
  let mut footnotes: Vec<LineFootnote> = Vec::new();
  let mut index_marks: Vec<LineIndexEntry> = Vec::new();
  let mut x = Length::ZERO;
  let mut height = Length::ZERO;
  let mut depth = Length::ZERO;
  for item in items {
    match item {
      HItem::Box(hbox) => {
        boxes.push(PositionedBox {
          content: hbox.content.clone(),
          x,
          dy: Length::ZERO,
          width: hbox.width,
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
      // 右寄せ末尾ボックス: 行内累積 x を無視し、本文幅の右端へ寄せる
      HItem::FlushRight(hbox) => {
        let flush_x = (available - hbox.width).max(Length::ZERO);
        boxes.push(PositionedBox {
          content: hbox.content.clone(),
          x: flush_x,
          dy: Length::ZERO,
          width: hbox.width,
        });
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
      // 脚注マーカーは幅 0・分割不可。行に積むだけで、この行の脚注として収集する
      // （ページ下部への行分割・配置は `break_pages` の責務）
      HItem::Footnote {
        number,
        index,
        items,
        leading,
      } => footnotes.push(LineFootnote {
        number: *number,
        index: *index,
        items: items.clone(),
        leading: *leading,
      }),
      // 索引マーカーは幅 0・分割不可。行に積むだけで、この行の索引語として収集する
      // （重複除去・ページ確定座標化は break_pages の責務）
      HItem::IndexMark { word, reading } => index_marks.push(LineIndexEntry {
        word: word.clone(),
        reading: reading.clone(),
      }),
      // 行内の Discretionary は描画しない（折り返し位置のハイフンは trailing_hyphen で出す）
      HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak => {},
    }
  }
  // 語中で折り返した行は、行内アイテムの直後（両端揃えでは伸縮後の右端）にハイフンを置く
  if let Some(hyphen) = trailing_hyphen {
    boxes.push(PositionedBox {
      content: hyphen.content.clone(),
      x,
      dy: Length::ZERO,
      width: hyphen.width,
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
  return Line {
    boxes,
    height,
    depth,
    is_last,
    links,
    footnotes,
    index_marks,
  };
}

/// 行分割テストの共有フィクスチャ（[`greedy`] / [`knuth_plass`] 両モジュールのテストが使う）
#[cfg(test)]
pub(super) mod test_support {
  use model::Length;

  use crate::typeset::layout::{HBox, HBoxContent, HItem};

  /// pt 値から `Length` を作る短縮子（テスト可読性のため）
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// テスト用の合成ボックス（幅 10、高さ 8、深さ 2）
  pub(super) fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: pt(10.0),
        height: pt(1.0),
      },
      width: pt(10.0),
      height: pt(8.0),
      depth: pt(2.0),
    });
  }

  /// テスト用の指定幅ボックス（高さ 8・深さ 2）
  pub(super) fn box_width(width: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: pt(width),
        height: pt(1.0),
      },
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
      content: HBoxContent::Rule {
        width: pt(width),
        height: pt(1.0),
      },
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
        content: HBoxContent::Rule {
          width: pt(hyphen_width),
          height: pt(1.0),
        },
        width: pt(hyphen_width),
        height: pt(6.0),
        depth: Length::ZERO,
      },
    };
  }

  /// テスト用の内部リンク行き先
  pub(super) fn link_target() -> model::LinkTarget {
    return model::LinkTarget::Internal(model::AnchorId::Label(model::LabelId::new("sec:x")));
  }

  /// テスト用の索引マーカー（幅 0・分割不可）
  pub(super) fn index_mark(word: &str, reading: Option<&str>) -> HItem {
    return HItem::IndexMark {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    };
  }
}
