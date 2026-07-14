//! Knuth–Plass 方式の段落全体最適な行分割
//!
//! 貪欲法（[`GreedyBreaker`]）が行ごとの局所判断で分割点を選ぶのに対し、本モジュールは
//! 段落全体で分割点の組を選び、各行の伸縮量から求めた badness に基づく総 demerits を最小化する。
//! 両端揃え（[`TextAlignment::Justify`]）でのみ最適化し、左揃え（`RaggedRight`。中央 / 右寄せ
//! 段落を含む）は従来どおり貪欲法へ委譲する。
//!
//! アルゴリズムは合法破断点を節とする最短経路 DP。行の確定・glue 配分・リンク矩形・右寄せ・
//! 行末ハイフンは貪欲法と共有の [`build_line`] に委ねるため、KP は分割点の集合を決めるだけで
//! 既存の意味（強制改行・証明終端マーク・リンク矩形・インライン数式）が保たれる。
//! 伸縮で収まらない病的な段落（分割不能な長箱等）は、そのサブ段落だけ貪欲法へフォールバックし、
//! 結果が貪欲法を下回らないことを保証する。

use model::{HItem, Length, Line, TextAlignment};

use super::{GreedyBreaker, LineBreaker, OpenLink, build_line, glue_metrics, strip_leading_glue, trim_trailing_glue};

/// 1 行ぶんの demerits に加える基礎ペナルティ（TeX の `\linepenalty` 相当）
const LINE_PENALTY: f64 = 10.0;
/// 語中ハイフネーションで折り返した行に課すペナルティ（TeX の `\hyphenpenalty` 相当）
const HYPHEN_PENALTY: f64 = 50.0;
/// 連続する行末でハイフンが続くときに加える追加 demerits（TeX の `\doublehyphendemerits` 相当）
const DOUBLE_HYPHEN_DEMERIT: f64 = 10_000.0;
/// badness の上限。極端に疎な行（伸長比が大きい行）はこの値で頭打ちにする
const INFINITE_BADNESS: f64 = 10_000.0;

/// Knuth–Plass 方式（段落全体最適）による行分割
///
/// [`TextAlignment::Justify`] のときのみ最適化し、それ以外は [`GreedyBreaker`] へ委譲する。
#[derive(Debug, Clone, Copy, Default)]
pub struct KnuthPlassBreaker;

impl LineBreaker for KnuthPlassBreaker {
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line> {
    // 両端揃え以外（大域設定 ragged_right / 中央・右寄せ段落）は従来の貪欲法のまま
    if alignment != TextAlignment::Justify {
      return GreedyBreaker.break_lines(items, text_width, alignment);
    }

    // 強制改行（ForcedBreak）で独立サブ段落に分割し、各々を最適化する。
    // 各サブ段落の最終行は is_last となり伸縮しない（強制改行直前の行の意味を保つ）。
    let segments = split_on_forced_break(items);
    let segment_count = segments.len();
    let mut lines: Vec<Line> = Vec::new();
    // 折り返しをまたいで開いているリンク領域（サブ段落・行をまたいで引き継ぐ）
    let mut open_links: Vec<OpenLink> = Vec::new();

    for (index, segment) in segments.iter().enumerate() {
      let has_following_break = index + 1 < segment_count;
      let is_sole_segment = segment_count == 1;
      // 末尾（強制改行の後ろ）の空サブ段落は行を作らない（貪欲法の末尾フラッシュと同じ挙動）
      if !has_following_break && !is_sole_segment && segment.is_empty() {
        continue;
      }
      let segment_lines = break_subparagraph(segment, text_width, &mut open_links);
      lines.extend(segment_lines);
    }
    return lines;
  }
}

/// 強制改行（[`HItem::ForcedBreak`]）位置で分割したサブ段落スライス列を返す
///
/// 強制改行アイテム自体は行を分けるだけで各スライスから除く。区切りが `k` 個なら `k + 1` 個の
/// スライスを返す（空文書は空スライス 1 個、単独の強制改行は空スライス 2 個）。
fn split_on_forced_break(items: &[HItem]) -> Vec<&[HItem]> {
  let mut segments: Vec<&[HItem]> = Vec::new();
  let mut start = 0;
  for (i, item) in items.iter().enumerate() {
    if matches!(item, HItem::ForcedBreak) {
      segments.push(&items[start..i]);
      start = i + 1;
    }
  }
  segments.push(&items[start..]);
  return segments;
}

/// 合法破断点（行末になり得る位置）
struct Breakpoint {
  /// 破断アイテムの index。行はこの index の手前まで（破断アイテム自体は行から除く）。
  /// `is_end` の仮想破断点では `items.len()`（末尾の後ろ）
  at: usize,
  /// 語中ハイフネーション（[`HItem::Discretionary`]）での破断か
  hyphen: bool,
  /// サブ段落末尾の仮想破断点（強制・最終行）か
  is_end: bool,
}

/// 1 本の候補行（開始位置 → 破断点）のコスト評価結果
enum Edge {
  /// 実現可能。値は demerits
  Feasible(f64),
  /// このエッジ単体では組めない（伸縮点が無いのに余る＝孤立した長語など）。行頭を前へずらすと
  /// 空白を得て組めることがあるため早期打ち切りはしない
  Infeasible,
  /// 収縮能力を超えて溢れる（実現不能）。行頭を前へずらすと更に長くなるため早期打ち切りに使う
  Overflow,
}

/// 強制改行を含まない 1 サブ段落を最適分割する
///
/// 合法破断点を節とする最短経路 DP で総 demerits 最小の分割点集合を求め、各行を [`build_line`]
/// で確定する。実現可能な経路が無い場合はこのサブ段落だけ貪欲法へフォールバックする。
fn break_subparagraph(items: &[HItem], text_width: Length, open_links: &mut Vec<OpenLink>) -> Vec<Line> {
  // 合法破断点を列挙し、末尾に仮想の強制破断点（最終行）を足す
  let mut breaks: Vec<Breakpoint> = Vec::new();
  for (i, item) in items.iter().enumerate() {
    match item {
      HItem::Glue {
        breakable: true, ..
      } => breaks.push(Breakpoint {
        at: i,
        hyphen: false,
        is_end: false,
      }),
      HItem::Penalty { value } if *value <= 0 => breaks.push(Breakpoint {
        at: i,
        hyphen: false,
        is_end: false,
      }),
      HItem::Discretionary { .. } => breaks.push(Breakpoint {
        at: i,
        hyphen: true,
        is_end: false,
      }),
      _ => {},
    }
  }
  breaks.push(Breakpoint {
    at: items.len(),
    hyphen: false,
    is_end: true,
  });

  // DP: best[j] = 破断点 breaks[j] で行を終える最小総 demerits。
  // prev[j] = Some(i) で直前破断が breaks[i]、None で行頭（item 0）から始まる（best[j] 有限時のみ有効）。
  let node_count = breaks.len();
  let mut best = vec![f64::INFINITY; node_count];
  let mut prev: Vec<Option<usize>> = vec![None; node_count];

  for j in 0..node_count {
    // 行頭候補を近い側（短い行）から見て、溢れたら以降（より長い行）は不要なので打ち切る
    let mut overflowed = false;
    for i in (0..j).rev() {
      let line_start = breaks[i].at + 1;
      match edge_cost(items, line_start, &breaks[j], breaks[i].hyphen, text_width) {
        Edge::Overflow => {
          overflowed = true;
          break;
        },
        Edge::Infeasible => {},
        Edge::Feasible(demerits) => {
          if best[i].is_finite() {
            let total = best[i] + demerits;
            if total < best[j] {
              best[j] = total;
              prev[j] = Some(i);
            }
          }
        },
      }
    }
    // START エッジ（行頭 = item 0 から breaks[j] まで）。より長い行なので溢れ済みなら不要
    if !overflowed
      && let Edge::Feasible(demerits) = edge_cost(items, 0, &breaks[j], false, text_width)
      && demerits < best[j]
    {
      best[j] = demerits;
      prev[j] = None;
    }
  }

  // 末尾の仮想破断点へ到達できない（実現可能な分割が無い）ならフォールバック
  if !best[node_count - 1].is_finite() {
    return GreedyBreaker.break_lines(items, text_width, TextAlignment::Justify);
  }

  // 逆リンクを辿って破断点列（行末順）を復元する
  let mut chain: Vec<usize> = Vec::new();
  let mut node = node_count - 1;
  loop {
    chain.push(node);
    match prev[node] {
      Some(i) => node = i,
      None => break,
    }
  }
  chain.reverse();

  // 破断点列から各行を build_line で確定する（open_links を行順に引き継ぐ）
  let mut lines: Vec<Line> = Vec::new();
  let mut line_start = 0usize;
  for &node in &chain {
    let brk = &breaks[node];
    let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
    let refs = strip_leading_glue(&refs);
    let trailing_hyphen = match items.get(brk.at) {
      Some(HItem::Discretionary { hyphen }) => Some(hyphen),
      _ => None,
    };
    lines.push(build_line(refs, brk.is_end, text_width, TextAlignment::Justify, open_links, trailing_hyphen));
    line_start = brk.at + 1;
  }
  return lines;
}

/// 候補行（`items[line_start..brk.at]`）の demerits を評価する
///
/// 行頭・行末の breakable glue を落とした実効内容から自然幅・伸縮能力を求め、非クランプの調整比
/// `r` に基づく badness `100·|r|³`（上限 [`INFINITE_BADNESS`]）から demerits を算出する。収縮能力を
/// 超えて溢れる行は [`Edge::Overflow`]、伸縮点が無いのに余る行（孤立した長語など）は
/// [`Edge::Infeasible`]（いずれも実現不能）。最終行（`is_end`）は伸縮しないため badness 0。
fn edge_cost(items: &[HItem], line_start: usize, brk: &Breakpoint, prev_hyphen: bool, text_width: Length) -> Edge {
  let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
  let refs = strip_leading_glue(&refs);
  let refs = trim_trailing_glue(refs);

  let (natural, stretch, shrink) = glue_metrics(refs);
  // FlushRight（QED）は右端を占有するため、収まり判定では幅に数える（build_line の自然幅からは除外済み）
  let flush_width: Length = refs
    .iter()
    .filter_map(|item| match item {
      HItem::FlushRight(hbox) => Some(hbox.width),
      _ => None,
    })
    .sum();
  // 語中破断は行末ハイフンぶん本文幅を狭める
  let hyphen_width = match items.get(brk.at) {
    Some(HItem::Discretionary { hyphen }) => hyphen.width,
    _ => Length::ZERO,
  };
  let available = text_width - hyphen_width;
  let leftover = available - (natural + flush_width);

  // 最終行は build_line が左揃え（伸縮なし）で組むため、自然幅で収まらなければ実現不能。
  // 収まってさえいれば疎密を罰しない（badness 0）。溢れは早期打ち切り対象（行頭を前へずらすと更に長い）。
  if brk.is_end {
    if leftover < Length::ZERO {
      return Edge::Overflow;
    }
    return Edge::Feasible(demerits(0.0, brk.hyphen, prev_hyphen));
  }

  // 非最終行は収縮を使える。収縮能力を超えて溢れる行は実現不能（行頭を前へずらすと更に長いので打ち切り対象）
  let overflows = leftover < Length::ZERO && (!shrink.is_positive() || leftover.ratio(shrink) < -1.0);
  if overflows {
    return Edge::Overflow;
  }

  // 非最終行の調整比。伸縮点が無いのに余る行は両端揃えできない（孤立した長語など）→ 実現不能。
  // このケースは行頭を前へずらして空白を得れば組めることがあるので早期打ち切りにはしない。
  let ratio: f64 = match leftover.cmp(&Length::ZERO) {
    std::cmp::Ordering::Greater => {
      if !stretch.is_positive() {
        return Edge::Infeasible;
      }
      leftover.ratio(stretch)
    },
    std::cmp::Ordering::Less => leftover.ratio(shrink),
    std::cmp::Ordering::Equal => 0.0,
  };

  let badness = (100.0 * ratio.abs().powi(3)).min(INFINITE_BADNESS);
  return Edge::Feasible(demerits(badness, brk.hyphen, prev_hyphen));
}

/// badness と破断種別から 1 行ぶんの demerits を求める
///
/// `(LINE_PENALTY + badness)²` を基礎に、語中ハイフネーション破断には [`HYPHEN_PENALTY`]²、
/// 直前もハイフン破断だった場合はさらに [`DOUBLE_HYPHEN_DEMERIT`] を加える。
fn demerits(badness: f64, hyphen: bool, prev_hyphen: bool) -> f64 {
  let mut total = (LINE_PENALTY + badness).powi(2);
  if hyphen {
    total += HYPHEN_PENALTY * HYPHEN_PENALTY;
    if prev_hyphen {
      total += DOUBLE_HYPHEN_DEMERIT;
    }
  }
  return total;
}

#[cfg(test)]
mod tests {
  use model::{HItem, Length, TextAlignment};

  use super::{
    super::test_support::{box_width, discretionary, flush_right_box, link_target, stretch_glue, test_box},
    GreedyBreaker, KnuthPlassBreaker, LineBreaker, break_subparagraph,
  };

  /// 行の右端（box 群の最大右端）
  fn right_edge(line: &model::Line) -> Length {
    return line.boxes.iter().map(|b| b.x + b.width).fold(Length::ZERO, Length::max);
  }

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に（sp 丸め精度内で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 2 つの `Length` が（sp 丸め精度内で）一致するか
  fn close_l(a: Length, b: Length) -> bool { return (a - b).abs() <= Length::from_sp(1); }

  #[test]
  fn ragged_right_delegates_to_greedy() {
    // Arrange — 伸縮能力付き glue でも ragged_right では貪欲法と完全一致の出力
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let kp = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::RaggedRight);
    let greedy = GreedyBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::RaggedRight);

    // Assert — 行数・各 box の x が一致
    assert_eq!(kp.len(), greedy.len(), "kp: {kp:?}, greedy: {greedy:?}");
    for (kp_line, greedy_line) in kp.iter().zip(&greedy) {
      assert_eq!(kp_line.boxes.len(), greedy_line.boxes.len());
      for (a, b) in kp_line.boxes.iter().zip(&greedy_line.boxes) {
        assert!(close_l(a.x, b.x), "kp: {kp:?}, greedy: {greedy:?}");
      }
    }
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    // 空段落は空の最終行 1 本
    let lines = KnuthPlassBreaker.break_lines(&[], Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
    assert!(lines[0].is_last);
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    // 1 行に収まるなら 1 行・最終行（伸縮しない）
    let items = vec![test_box(), stretch_glue(), test_box()];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].is_last);
    // 最終行なので glue は自然幅 5（box は 0 と 15）
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_flushes_non_final_line_to_right_edge() {
    // Arrange — box(10) glue(5±) box(10) glue(5±) box(10)、text_width=27。
    // 非最終行の右端が版面右端に一致する
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert — 2 行、1 行目の右端 = 27
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(!lines[0].is_last);
    assert!(close(right_edge(&lines[0]), 27.0), "非最終行の右端は版面右端: {lines:?}");
  }

  #[test]
  fn uniform_density_beats_greedy_loose_lines() {
    // Arrange — 貪欲法は収まり判定を自然幅だけで行い収縮を使わないため、
    // あと少し詰めれば 3 個乗る行でも 2 個で折り返し、非最終行が疎になる。
    // box(8) を glue(natural=2, stretch=1, shrink=2) で 5 個、text_width=25:
    //   greedy: 3 個目の自然幅 28 > 25 で 2 個ずつ折り返し → (2,2,1)。
    //           非最終行は伸長能力 1 に対し余り 7 で、上限クランプ後も右端 19（< 25）と疎。
    //   KP: 収縮を使い 1 行目に 3 個（自然幅 28 → 収縮 -0.75 で 25 に収める）→ (3,2)。
    //       非最終行の右端が版面右端 25 に一致し、行の疎密が均一化する。
    let glue = || HItem::Glue {
      natural: pt(2.0),
      stretch: pt(1.0),
      shrink: pt(2.0),
      breakable: true,
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

    // Act
    let greedy = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);
    let kp = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert — greedy は 3 行（非最終行が疎で右端が版面右端に届かない）
    assert_eq!(greedy.len(), 3, "greedy: {greedy:?}");
    assert!(!greedy[0].is_last);
    assert!(right_edge(&greedy[0]) < pt(24.0), "greedy の非最終行は疎（右端 < 25）: {greedy:?}");

    // KP は 2 行に詰め、非最終行の右端が版面右端に一致する（疎密が均一）
    assert_eq!(kp.len(), 2, "kp: {kp:?}");
    assert!(!kp[0].is_last);
    assert!(close(right_edge(&kp[0]), 25.0), "KP の非最終行は右端に揃う: {kp:?}");
  }

  #[test]
  fn forced_break_line_is_last_and_not_stretched() {
    // Arrange — 強制改行直前の行は is_last となり伸縮しない
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert — 2 行、どちらも is_last。1 行目の glue は自然幅のまま（box は 0 と 15）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].is_last);
    assert!(lines[1].is_last);
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn empty_segment_before_forced_break_yields_empty_line() {
    // 強制改行の前が空なら空行を 1 本作る（貪欲法と同じ）
    let items = vec![HItem::ForcedBreak, test_box()];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].boxes.is_empty(), "1 行目は空: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn trailing_forced_break_does_not_add_empty_line() {
    // 末尾の強制改行の後ろには空行を作らない
    let items = vec![test_box(), HItem::ForcedBreak];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!(lines[0].is_last);
  }

  #[test]
  fn flush_right_box_wraps_when_it_does_not_fit() {
    // Arrange — box(10) Penalty(0) FlushRight(8)、text_width=14。QED は同居できず次行右端へ
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(14.0), TextAlignment::Justify);

    // Assert — 2 行。2 行目に QED だけが右端（14 − 8 = 6）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].x, 6.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    // box(10) glue(5±) FlushRight(8)、text_width=50 なら最終行の右端に収まる
    let items = vec![test_box(), stretch_glue(), flush_right_box(8.0)];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(50.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED: {lines:?}");
    assert!(close(lines[0].boxes[1].x, 42.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn link_rect_follows_stretched_glue() {
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
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert — 1 行目の矩形右端は伸長後の右端 27
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!(close(lines[0].links[0].x1, 27.0), "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn inline_atom_box_is_never_split() {
    // 段幅より広い単一 box（インライン数式 Atom 相当）は分割されず 1 行に overflow する。
    // 分割点が無く末尾仮想破断点へ到達できないため貪欲法へフォールバックし、同じ結果になる
    let items = vec![box_width(50.0)];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!(close(lines[0].boxes[0].width, 50.0));
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    // Arrange — box(20) disc(h=3) box(20)、text_width=25。1 行に収まらず disc で折り返し、
    // 行末にハイフン箱が付く（KP がハイフン破断を選ぶ）
    let items = vec![box_width(20.0), discretionary(3.0), box_width(20.0)];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert — 2 行。1 行目末尾にハイフン（幅 3）が付く
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box + 行末ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[1].width, 3.0), "行末ハイフン: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn avoids_hyphen_break_when_space_break_available() {
    // Arrange — box(10) space(5±) box(10) disc(h=3) box(10)、text_width=25。
    // スペースでもハイフンでも折り返せるが、HYPHEN_PENALTY によりスペース破断を選ぶ
    // （行末にハイフンが付かない）
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert — 2 行。1 行目末尾にハイフン箱が付かない（スペースで折り返した）
    assert_eq!(lines.len(), 2, "{lines:?}");
    let has_hyphen = lines[0].boxes.len() > 1 && close(lines[0].boxes.last().unwrap().width, 3.0);
    assert!(!has_hyphen, "不要なハイフンを避ける: {lines:?}");
  }

  #[test]
  fn no_feasible_path_falls_back_to_greedy() {
    // Arrange — 分割点が Penalty(0) しかなく、どの行も伸縮点を持たない（box + Penalty 連結）。
    // box(40) が段幅 30 を超えるが分割不能な行が生じ得る配置でも panic せず、貪欲法と一致する
    let items = vec![
      box_width(40.0),
      HItem::Penalty { value: 0 },
      box_width(40.0),
    ];

    // Act
    let kp = KnuthPlassBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);
    let greedy = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    // Assert — 行数・各 box の x が貪欲法と一致（下回らない）
    assert_eq!(kp.len(), greedy.len(), "kp: {kp:?}, greedy: {greedy:?}");
    for (kp_line, greedy_line) in kp.iter().zip(&greedy) {
      assert_eq!(kp_line.boxes.len(), greedy_line.boxes.len(), "kp: {kp:?}, greedy: {greedy:?}");
    }
  }

  #[test]
  fn break_subparagraph_marks_only_last_line_is_last() {
    // Arrange — 3 行に分かれる十分長い列で、最終行だけが is_last になる
    let items = vec![
      box_width(20.0),
      stretch_glue(),
      box_width(20.0),
      stretch_glue(),
      box_width(20.0),
    ];

    // Act
    let mut open_links = Vec::new();
    let lines = break_subparagraph(&items, Length::pt(22.0), &mut open_links);

    // Assert — 3 行、最終行のみ is_last
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(!lines[0].is_last);
    assert!(!lines[1].is_last);
    assert!(lines[2].is_last);
  }
}
