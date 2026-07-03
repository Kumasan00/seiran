//! (c) 行分割 — 貪欲法（first-fit）
//!
//! [`LineBreaker`] トレイトの実装として [`GreedyBreaker`] を提供する。
//! box は計測済みの幅を持つため、行分割はフォントに一切触れない純粋計算になる。
//! 分割点の選択は常に自然幅で行い、両端揃え（[`TextAlignment::Justify`]）は
//! 確定した行内の伸縮点（glue）の幅だけを変える仕上げの工程として適用する。
//! 将来 Knuth-Plass を導入する際は入出力型を変えずに実装を追加する。

use types::{LinkTarget, TextAlignment};

use crate::{
  hitem::{HBox, HItem},
  line::{Line, LineLink, PositionedBox},
};

/// 行分割アルゴリズムの抽象
pub trait LineBreaker {
  /// 水平リストを本文幅で行に分割する
  ///
  /// `text_width` は本文幅（pt）。分割点が存在しない場合は overflow を許容する。
  /// `alignment` は行末処理で、[`TextAlignment::Justify`] のとき段落最終行・強制改行
  /// 直前の行を除く各行の余り幅を伸縮点へ比例配分する（分割点の選択には影響しない）。
  fn break_lines(&self, items: &[HItem], text_width: f32, alignment: TextAlignment) -> Vec<Line>;
}

/// 行分割中に開いている（`LinkStart` 済み・`LinkEnd` 未到達）リンク領域の状態
///
/// 折り返しをまたいで継続するため、`break_lines` のループ全体で 1 つ保持し、
/// 各行（[`build_line`]）に渡して引き継ぐ。
struct OpenLink {
  /// リンクの行き先
  target: LinkTarget,
  /// 現在の行における領域左端の行頭からの水平オフセット（pt）
  x0: f32,
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
  fn break_lines(&self, items: &[HItem], text_width: f32, alignment: TextAlignment) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    // 現在の行に積んだアイテム
    let mut buffer: Vec<&HItem> = Vec::new();
    // 現在の行の自然幅
    let mut width_so_far = 0.0f32;
    // 直近の分割可能点（buffer 内インデックス）
    let mut last_break: Option<usize> = None;
    // 折り返しをまたいで開いているリンク領域（行間で引き継ぐ）
    let mut open_links: Vec<OpenLink> = Vec::new();

    for item in items {
      match item {
        HItem::ForcedBreak => {
          lines.push(build_line(&buffer, true, text_width, alignment, &mut open_links, None));
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
        HItem::Discretionary { hyphen } => {
          buffer.push(item);
          // ハイフンを足しても本文幅に収まる語中点だけを分割候補にする
          // （折り返すと行末にハイフンが乗るため、右端超過を作らない）。自然幅には寄与しない
          if width_so_far + hyphen.width <= text_width {
            last_break = Some(buffer.len() - 1);
          }
        },
        // リンクマーカーは幅 0・分割不可。行に積むだけで build_line が矩形を収集する
        HItem::LinkStart(_) | HItem::LinkEnd => {
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
            lines.push(build_line(
              &buffer[..break_index],
              false,
              text_width,
              alignment,
              &mut open_links,
              trailing_hyphen,
            ));
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
      lines.push(build_line(&buffer, true, text_width, alignment, &mut open_links, None));
    }
    return lines;
  }
}

/// 行の余り幅から glue の伸縮係数を求める（両端揃えの配分係数）
///
/// 正の値は伸長（各 glue の `stretch` に乗算）、負の値は収縮（`shrink` に乗算）を表し、
/// 大きさは 1.0 でクランプする。行の余り幅が伸縮能力を超える場合は上限で止まり、
/// 右端不一致を許容する（貪欲法では発生しうる。Knuth-Plass で解消対象）。
/// 伸縮点が存在しない行（`stretch` / `shrink` の総和が 0）は 0 を返し、左揃えのまま置く。
/// `FlushRight` は行内の x 進行を消費しない（右端へ独立に寄る）ため自然幅の合計から除く。
fn glue_adjust_ratio(items: &[&HItem], available: f32) -> f32 {
  let natural: f32 = items
    .iter()
    .filter(|item| !matches!(item, HItem::FlushRight(_)))
    .map(|item| item.natural_width())
    .sum();
  let leftover = available - natural;
  if leftover > 0.0 {
    let total_stretch: f32 = items
      .iter()
      .map(|item| match item {
        HItem::Glue { stretch, .. } => *stretch,
        _ => 0.0,
      })
      .sum();
    if total_stretch <= 0.0 {
      return 0.0;
    }
    return (leftover / total_stretch).min(1.0);
  }
  if leftover < 0.0 {
    let total_shrink: f32 = items
      .iter()
      .map(|item| match item {
        HItem::Glue { shrink, .. } => *shrink,
        _ => 0.0,
      })
      .sum();
    if total_shrink <= 0.0 {
      return 0.0;
    }
    return -((-leftover / total_shrink).min(1.0));
  }
  return 0.0;
}

/// アイテム列から 1 行を組み立てる（位置確定）
///
/// 行末の breakable glue は破棄する。`Penalty` は幅を持たないため位置決めに影響しない。
/// `open_links` は折り返しをまたいで開いているリンク領域の状態で、`LinkStart` / `LinkEnd`
/// に応じて更新しつつ、この行に属するクリック矩形（[`LineLink`]）を収集する。
/// `available` は本文幅（折り返し幅）で、`FlushRight` を `available − 幅` に右寄せするのに使う。
///
/// `alignment` が [`TextAlignment::Justify`] かつ最終行（段落最終行・強制改行直前）でない
/// とき、行の余り幅を各 glue の伸縮能力に比例して配分し行末を右端に揃える。リンク矩形は
/// 伸縮後の x を読むため字位置に自動追従する。
///
/// `trailing_hyphen` が `Some` のとき（語中 [`HItem::Discretionary`] で折り返した行）、
/// 行内アイテムを配置した後の行末にハイフン箱を置く。両端揃えではハイフン幅を差し引いた
/// 余り幅で glue を伸縮させ、ハイフン込みで右端に揃える。
fn build_line(
  items: &[&HItem],
  is_last: bool,
  available: f32,
  alignment: TextAlignment,
  open_links: &mut Vec<OpenLink>,
  trailing_hyphen: Option<&HBox>,
) -> Line {
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

  // 両端揃えの配分係数（正 = 伸長 / 負 = 収縮）。最終行は伸縮しない。
  // 行末ハイフンぶんを本文幅から差し引いた余り幅で配分する（ハイフン込みで右端に揃える）
  let hyphen_width = trailing_hyphen.map_or(0.0, |hyphen| hyphen.width);
  let adjust_ratio = if alignment == TextAlignment::Justify && !is_last {
    glue_adjust_ratio(items, available - hyphen_width)
  } else {
    0.0
  };

  // 前の行から継続するリンクは、この行では行頭（x0 = 0）から始まる
  for open in open_links.iter_mut() {
    open.x0 = 0.0;
  }

  let mut boxes: Vec<PositionedBox> = Vec::new();
  let mut links: Vec<LineLink> = Vec::new();
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
      HItem::Glue {
        natural,
        stretch,
        shrink,
        ..
      } => {
        x += natural
          + if adjust_ratio >= 0.0 {
            adjust_ratio * stretch
          } else {
            adjust_ratio * shrink
          };
      },
      HItem::Kern(value) => x += value,
      // 右寄せ末尾ボックス: 行内累積 x を無視し、本文幅の右端へ寄せる
      HItem::FlushRight(hbox) => {
        let flush_x = (available - hbox.width).max(0.0);
        boxes.push(PositionedBox {
          content: hbox.content.clone(),
          x: flush_x,
          dy: 0.0,
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
      // 行内の Discretionary は描画しない（折り返し位置のハイフンは trailing_hyphen で出す）
      HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak => {},
    }
  }
  // 語中で折り返した行は、行内アイテムの直後（両端揃えでは伸縮後の右端）にハイフンを置く
  if let Some(hyphen) = trailing_hyphen {
    boxes.push(PositionedBox {
      content: hyphen.content.clone(),
      x,
      dy: 0.0,
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
  };
}

#[cfg(test)]
mod tests {
  use types::TextAlignment;

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

  /// テスト用の右寄せ末尾ボックス（QED 相当・指定幅、高さ 6・深さ 0）
  fn flush_right_box(width: f32) -> HItem {
    return HItem::FlushRight(HBox {
      content: HBoxContent::Rule { width, height: 1.0 },
      width,
      height: 6.0,
      depth: 0.0,
    });
  }

  /// テスト用の伸縮能力付き breakable glue（幅 5・伸長 2.5・収縮 5/3 = 単語間スペース相当）
  fn stretch_glue() -> HItem {
    return HItem::Glue {
      natural: 5.0,
      stretch: 2.5,
      shrink: 5.0 / 3.0,
      breakable: true,
    };
  }

  /// テスト用の伸縮能力付き非 breakable glue（分割点にならず overflow 行を作れる）
  fn non_breakable_stretch_glue() -> HItem {
    return HItem::Glue {
      natural: 5.0,
      stretch: 2.5,
      shrink: 5.0 / 3.0,
      breakable: false,
    };
  }

  /// テスト用の和文字間 glue（幅 0・伸長 0.5・収縮なし = フォントサイズ 10pt の字間相当）
  fn cjk_glue() -> HItem {
    return HItem::Glue {
      natural: 0.0,
      stretch: 0.5,
      shrink: 0.0,
      breakable: true,
    };
  }

  /// テスト用の語中ハイフネーション分割点（指定幅のハイフン箱・高さ 6・深さ 0）
  fn discretionary(hyphen_width: f32) -> HItem {
    return HItem::Discretionary {
      hyphen: HBox {
        content: HBoxContent::Rule {
          width: hyphen_width,
          height: 1.0,
        },
        width: hyphen_width,
        height: 6.0,
        depth: 0.0,
      },
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
    let lines = GreedyBreaker.break_lines(&items, 30.0, TextAlignment::RaggedRight);

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
    let lines = GreedyBreaker.break_lines(&items, 30.0, TextAlignment::RaggedRight);

    // Assert — 1 行目: box(0..10) glue(10..15) box(15..25)。2 つ目の box の x は 15
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    // 幅が十分なら 1 行に収まり、is_last = true
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

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
    let lines = GreedyBreaker.break_lines(&items, 25.0, TextAlignment::RaggedRight);

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

    let lines = GreedyBreaker.break_lines(&items, 25.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "分割点がなければ overflow 許容: {lines:?}");
    assert_eq!(lines[0].boxes.len(), 3);
  }

  #[test]
  fn forced_break_flushes_line_unconditionally() {
    // ForcedBreak は幅に関係なく行を確定し、is_last = true になる
    let items = vec![test_box(), HItem::ForcedBreak, test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 2);
    assert!(lines[0].is_last, "強制改行の行は is_last: {lines:?}");
    assert!(lines[1].is_last);
  }

  #[test]
  fn line_height_and_depth_from_boxes() {
    // 行の height / depth は行内ボックスの最大値
    let items = vec![test_box(), space_glue(), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

    assert!((lines[0].height - 8.0).abs() < f32::EPSILON);
    assert!((lines[0].depth - 2.0).abs() < f32::EPSILON);
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    // 空の段落も 1 行（空行）として返す
    let lines = GreedyBreaker.break_lines(&[], 100.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
    assert!(lines[0].is_last);
  }

  #[test]
  fn kern_is_not_a_break_opportunity_and_is_kept() {
    // Kern は分割点にならず、行末でも破棄されない（幅に寄与する）
    let items = vec![test_box(), HItem::Kern(5.0), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1);
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    // Arrange — box(10) glue(5) FlushRight(8)、text_width=50 なら最終語の後ろに収まる
    let items = vec![test_box(), space_glue(), flush_right_box(8.0)];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 50.0, TextAlignment::RaggedRight);

    // Assert — 1 行。本文 box は x=0、FlushRight は右端（50 − 8 = 42）に寄る
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED box の 2 つ: {lines:?}");
    assert!((lines[0].boxes[0].x - 0.0).abs() < f32::EPSILON);
    assert!((lines[0].boxes[1].x - 42.0).abs() < f32::EPSILON, "QED は右端寄せ: {lines:?}");
    assert!(lines[0].is_last);
  }

  #[test]
  fn flush_right_box_wraps_to_next_line_when_it_does_not_fit() {
    // Arrange — box(10) Penalty(0) FlushRight(8)、text_width=14 では同居できず次行へ。
    // Penalty(0) が直前の分割機会となり、QED だけが折り返す（本文 box は残る）
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 14.0, TextAlignment::RaggedRight);

    // Assert — 2 行。1 行目は本文 box のみ、2 行目は QED だけが右端（14 − 8 = 6）に
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert!((lines[0].boxes[0].x - 0.0).abs() < f32::EPSILON);
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED box のみ: {lines:?}");
    assert!((lines[1].boxes[0].x - 6.0).abs() < f32::EPSILON, "QED は右端寄せ: {lines:?}");
    assert!(lines[1].is_last);
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

    let lines = GreedyBreaker.break_lines(&items, 30.0, TextAlignment::RaggedRight);

    for line in &lines {
      let width = line.boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
      assert!(width <= 30.0 + f32::EPSILON, "行幅 {width} が段幅 30 を超えた: {line:?}");
    }
  }

  /// テスト用の内部リンク行き先
  fn link_target() -> types::LinkTarget { return types::LinkTarget::Internal("sec:x".to_string()); }

  #[test]
  fn link_markers_collect_single_rect_on_one_line() {
    // Arrange — LinkStart box box LinkEnd が 1 行に収まる（幅 100）
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

    // Assert — 1 行・1 矩形（x0=0, x1=20）。マーカーは幅 0 なので box 2 つ分
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].links.len(), 1);
    assert!((lines[0].links[0].x0 - 0.0).abs() < f32::EPSILON);
    assert!((lines[0].links[0].x1 - 20.0).abs() < f32::EPSILON, "{:?}", lines[0].links);
  }

  #[test]
  fn link_spanning_wrap_splits_into_two_rects() {
    // Arrange — LinkStart box glue box LinkEnd。幅 12 で box 2 つ目が折り返す
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      space_glue(),
      test_box(),
      HItem::LinkEnd,
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 12.0, TextAlignment::RaggedRight);

    // Assert — 2 行に分割され、各行に 1 矩形（どちらも x0=0, x1=10）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "1 行目に継続中の矩形: {:?}", lines[0].links);
    assert!((lines[0].links[0].x1 - 10.0).abs() < f32::EPSILON);
    assert_eq!(lines[1].links.len(), 1, "2 行目に残りの矩形: {:?}", lines[1].links);
    assert!((lines[1].links[0].x0 - 0.0).abs() < f32::EPSILON);
    assert!((lines[1].links[0].x1 - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn line_without_links_has_empty_links() {
    // リンクマーカーが無い段落の行は links が空
    let lines = GreedyBreaker.break_lines(&[test_box(), space_glue(), test_box()], 100.0, TextAlignment::RaggedRight);

    assert!(lines[0].links.is_empty());
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

    let lines = GreedyBreaker.break_lines(&[wide], 30.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!((lines[0].boxes[0].width - 50.0).abs() < f32::EPSILON);
  }

  #[test]
  fn justify_stretches_glue_to_flush_right_edge() {
    // Arrange — box(10) glue(5±) box(10) glue(5±) box(10)、text_width=27 で 2 つ目の glue で折り返す。
    // 1 行目の自然幅 25・余り 2 は伸長能力 2.5 の内側なので、glue が 7 に伸びて右端が 27 に一致する
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 27.0, TextAlignment::Justify);

    // Assert — 1 行目: box(0..10) glue(10..17) box(17..27)。右端 = 本文幅
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 17.0).abs() < 1e-4, "{lines:?}");
    let right_edge = lines[0].boxes[1].x + lines[0].boxes[1].width;
    assert!((right_edge - 27.0).abs() < 1e-4, "非最終行の右端は版面右端に一致: {lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_last_line() {
    // Arrange — 1 行に収まる（= 段落最終行）ので、余り幅があっても伸縮しない
    let items = vec![test_box(), stretch_glue(), test_box()];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 27.0, TextAlignment::Justify);

    // Assert — glue は自然幅 5 のまま（box は 0 と 15）
    assert_eq!(lines.len(), 1);
    assert!(lines[0].is_last);
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn justify_does_not_stretch_line_before_forced_break() {
    // Arrange — 強制改行（\\）直前の行は is_last となり伸縮しない
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 27.0, TextAlignment::Justify);

    // Assert — 1 行目の glue は自然幅のまま
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].is_last);
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_stretch_limit() {
    // Arrange — text_width=30 で 1 行目の余り 5 が伸長能力 2.5 を超える。
    // 上限で止まり（glue = 7.5）、右端不一致（27.5 < 30）を許容する
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 30.0, TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 17.5).abs() < 1e-4, "伸長は能力の上限で止まる: {lines:?}");
  }

  #[test]
  fn justify_shrinks_overfull_line() {
    // Arrange — box(10) 非分割glue(5±) box(10) は分割点がなく自然幅 25 のまま 1 行に確定する。
    // text_width=24 では超過 1 が収縮能力 5/3 の内側なので、glue が 4 に詰まり右端が 24 に一致する
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 24.0, TextAlignment::Justify);

    // Assert — 1 行目: box(0..10) glue(10..14) box(14..24)
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 14.0).abs() < 1e-4, "{lines:?}");
  }

  #[test]
  fn justify_clamps_at_shrink_limit() {
    // Arrange — text_width=23 では超過 2 が収縮能力 5/3 を超える。
    // 下限で止まり（glue = 5 − 5/3）、右端の超過（23.33 > 23）を許容する
    let items = vec![
      test_box(),
      non_breakable_stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 23.0, TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - (15.0 - 5.0 / 3.0)).abs() < 1e-4, "収縮は能力の下限で止まる: {lines:?}");
  }

  #[test]
  fn justify_leaves_line_without_stretch_points_ragged() {
    // Arrange — 1 行目が box kern box（伸縮点なし）。余り幅 1 があっても左揃えのまま
    let items = vec![
      test_box(),
      HItem::Kern(5.0),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 26.0, TextAlignment::Justify);

    // Assert — kern は伸縮せず box は自然位置（0 と 15）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn justify_moves_link_rects_with_stretched_glue() {
    // Arrange — リンクが伸長される glue をまたぐ。矩形は伸縮後の字位置に追従する
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
    let lines = GreedyBreaker.break_lines(&items, 27.0, TextAlignment::Justify);

    // Assert — 1 行目の矩形は x0=0, x1=27（glue 7 に伸長後の右端）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!((lines[0].links[0].x0 - 0.0).abs() < f32::EPSILON);
    assert!((lines[0].links[0].x1 - 27.0).abs() < 1e-4, "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn ragged_right_ignores_stretch_capacity() {
    // Arrange — 伸縮能力付き glue でも ragged_right では自然幅のまま並ぶ（従来と同一の出力）
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 27.0, TextAlignment::RaggedRight);

    // Assert — 1 行目: box(0..10) glue(10..15) box(15..25)
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 15.0).abs() < f32::EPSILON, "{lines:?}");
  }

  #[test]
  fn justify_stretches_cjk_zero_width_glue_to_flush_right_edge() {
    // Arrange — 和文行モデル: box(10) 字間glue(0+0.5) box(10) 字間glue(0+0.5) box(10)。
    // text_width=20.3 では 3 つ目の box が収まらず 2 つ目の字間 glue で折り返す。
    // 1 行目の自然幅 20・余り 0.3 は伸長能力 0.5 の内側なので、字間が 0.3 に開いて右端が一致する
    let items = vec![test_box(), cjk_glue(), test_box(), cjk_glue(), test_box()];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 20.3, TextAlignment::Justify);

    // Assert — 1 行目: box(0..10) 字間(10..10.3) box(10.3..20.3)。非最終行の右端 = 本文幅
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 10.3).abs() < 1e-4, "{lines:?}");
    let right_edge = lines[0].boxes[1].x + lines[0].boxes[1].width;
    assert!((right_edge - 20.3).abs() < 1e-4, "和文のみの非最終行の右端は版面右端に一致: {lines:?}");
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    // Arrange — box(10) disc(h=3) box(10) disc(h=3) box(10)、text_width=25 では
    // 3 つ目の box が収まらず 2 つ目の disc で折り返し、行末にハイフン箱が付く
    let items = vec![
      test_box(),
      discretionary(3.0),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 25.0, TextAlignment::RaggedRight);

    // Assert — 1 行目は box2 つ + ハイフン箱、2 行目は残りの box 1 つ
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 3, "本文 box 2 つ + 行末ハイフン: {lines:?}");
    // ハイフンは box2 の直後（x=20）に幅 3 で置かれ、右端 23 は本文幅 25 を超えない
    assert!((lines[0].boxes[2].x - 20.0).abs() < f32::EPSILON, "{lines:?}");
    assert!((lines[0].boxes[2].width - 3.0).abs() < f32::EPSILON, "{lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
    assert!(right_edge <= 25.0 + f32::EPSILON, "ハイフン込みで右端超過なし: {right_edge}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn discretionary_not_used_when_word_fits() {
    // 幅が十分なら語中では折り返さず、行末ハイフンも付かない（空白/語境界優先）
    let items = vec![test_box(), discretionary(3.0), test_box()];

    let lines = GreedyBreaker.break_lines(&items, 100.0, TextAlignment::RaggedRight);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "ハイフン箱は付かない: {lines:?}");
  }

  #[test]
  fn discretionary_rejected_when_hyphen_would_overflow() {
    // Arrange — box(10) disc(h=1) box(10) disc(h=20) box(10)、text_width=22。
    // 2 つ目の disc はハイフン(20)を足すと右端を超えるため分割候補にならず、
    // 収まる 1 つ目の disc(h=1) へ後退して折り返す
    let items = vec![
      test_box(),
      discretionary(1.0),
      test_box(),
      discretionary(20.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 22.0, TextAlignment::RaggedRight);

    // Assert — 1 行目は box1 + 幅 1 のハイフン（disc1 で折り返した証拠）、2 行目は box2 box3
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "box1 + ハイフン: {lines:?}");
    assert!((lines[0].boxes[1].width - 1.0).abs() < f32::EPSILON, "使われたのは disc1 のハイフン: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
  }

  #[test]
  fn justify_includes_hyphen_width_at_flush_right_edge() {
    // Arrange — box(10) 伸縮glue(5±2.5) box(10) disc(h=3) box(10)、text_width=29 で disc 折り返し。
    // 両端揃えはハイフン幅を差し引いた余り幅で glue を伸ばし、ハイフン込みで右端 29 に一致させる
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = GreedyBreaker.break_lines(&items, 29.0, TextAlignment::Justify);

    // Assert — glue が 6 に伸び box2 は x=16、行末ハイフン右端が本文幅 29 に一致
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!((lines[0].boxes[1].x - 16.0).abs() < 1e-4, "glue 伸長後の box2: {lines:?}");
    let right_edge = lines[0].boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
    assert!((right_edge - 29.0).abs() < 1e-4, "ハイフン込みで右端に揃う: {right_edge}");
  }

  #[test]
  fn cjk_zero_width_glue_breaks_like_zero_penalty() {
    // Arrange — 同じ box 列を Penalty(0) 繋ぎと幅 0 glue 繋ぎで分割する。
    // 幅 0 glue は自然幅に寄与しないため、分割位置・行内容・自然位置が Penalty(0) と一致する
    // （禁則を含む折り返し挙動が変わらないことの担保）。ragged_right では伸長も効かず出力同一
    let penalty_items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
      HItem::Penalty { value: 0 },
      test_box(),
    ];
    let glue_items = vec![test_box(), cjk_glue(), test_box(), cjk_glue(), test_box()];

    // Act
    let penalty_lines = GreedyBreaker.break_lines(&penalty_items, 25.0, TextAlignment::RaggedRight);
    let glue_lines = GreedyBreaker.break_lines(&glue_items, 25.0, TextAlignment::RaggedRight);

    // Assert — 行数・各行の box 数・各 box の x が完全一致
    assert_eq!(penalty_lines.len(), glue_lines.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
    for (penalty_line, glue_line) in penalty_lines.iter().zip(&glue_lines) {
      assert_eq!(penalty_line.boxes.len(), glue_line.boxes.len(), "penalty: {penalty_lines:?}, glue: {glue_lines:?}");
      for (penalty_box, glue_box) in penalty_line.boxes.iter().zip(&glue_line.boxes) {
        assert!(
          (penalty_box.x - glue_box.x).abs() < f32::EPSILON,
          "penalty: {penalty_lines:?}, glue: {glue_lines:?}"
        );
      }
    }
  }
}
