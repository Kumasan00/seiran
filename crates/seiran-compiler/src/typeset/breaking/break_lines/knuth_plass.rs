//! Knuth–Plass 方式の段落全体最適な行分割

use tracing::trace;

use crate::{
  length::Length,
  style::TextAlignment,
  typeset::{
    boxes::{HItem, Line},
    breaking::break_lines::{
      GreedyBreaker, LineBreaker, OpenLink, build_line, glue_metrics, strip_leading_glue, trim_trailing_glue,
    },
    observe,
  },
};

/// 1 行ぶんの demerits に加える基礎ペナルティ（TeX の `\linepenalty` 相当）
const LINE_PENALTY: f64 = 10.0;
/// 語中ハイフネーションで折り返した行に課すペナルティ（TeX の `\hyphenpenalty` 相当）
const HYPHEN_PENALTY: f64 = 50.0;
/// ハイフネーションで折り返した行 1 本ぶんの demerits（TeX と同じくハイフンペナルティの 2 乗）
const HYPHEN_DEMERIT: f64 = HYPHEN_PENALTY * HYPHEN_PENALTY;
/// 連続する行末でハイフンが続くときに加える追加 demerits（TeX の `\doublehyphendemerits` 相当）
const DOUBLE_HYPHEN_DEMERIT: f64 = 10_000.0;
/// badness の上限。極端に疎な行（伸長比が大きい行）はこの値で頭打ちにする
const INFINITE_BADNESS: f64 = 10_000.0;

/// Knuth–Plass 方式（段落全体最適）による行分割
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct KnuthPlassBreaker;

impl LineBreaker for KnuthPlassBreaker {
  fn break_lines(&self, items: &[HItem], text_width: Length, alignment: TextAlignment) -> Vec<Line> {
    // 両端揃え以外（大域設定 ragged_right / 中央・右寄せ段落）は従来の貪欲法のまま
    if alignment != TextAlignment::Justify {
      return GreedyBreaker.break_lines(items, text_width, alignment);
    }

    // 強制改行（ForcedBreak）で独立サブ段落に分割し、各々を最適化する。
    // 各サブ段落の最終行は伸縮しない（強制改行直前の行の意味を保つ）。
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
  /// 数式内分割点（[`HItem::MathBreak`]）での破断なら、そのペナルティ
  math_penalty: Option<i32>,
  /// サブ段落末尾の仮想破断点（強制・最終行）か
  is_end: bool,
}

/// 破断経路の累積コスト（DP が最小化する量）
///
/// 数式内分割点（[`HItem::MathBreak`]）は「他の分割点で組めないときだけ使う」ので、その使用回数を
/// demerits より優先して比べる（derive した `PartialOrd` はフィールドの宣言順に辞書式比較する）。
/// demerits への定数加算では表せない — どれほど大きな定数でも、疎な行が十分に続けば数式内分割の方が
/// 安くなってしまう。数式内分割点を含まない段落では `math_breaks` が常に 0 なので、比較は従来の
/// demerits 比較と一致する。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct PathCost {
  /// 経路上の数式内分割の回数
  math_breaks: u32,
  /// 経路上の demerits の総和
  demerits: f64,
}

impl PathCost {
  /// 経路の起点（行頭 = item 0）のコスト
  const START: PathCost = PathCost {
    math_breaks: 0,
    demerits: 0.0,
  };

  /// 破断点 `brk` で終わる 1 行（demerits `demerits`）を継ぎ足した経路のコストを返す
  fn then(self, brk: &Breakpoint, demerits: f64) -> Self {
    return PathCost {
      math_breaks: self.math_breaks + u32::from(brk.math_penalty.is_some()),
      demerits: self.demerits + demerits,
    };
  }
}

/// 1 本の候補行（開始位置 → 破断点）のコスト評価結果
///
/// `Feasible` が demerits だけでなく badness と調整比も持つのは、DP が最小化する [`PathCost`] へは
/// この行の demerits だけ渡せば足りる（数式内分割の回数は breakpoint 自身の `math_penalty` から
/// [`PathCost::then`] が決めるので、Edge 側は関与しない）一方で、TRACE 観測では「なぜその demerits に
/// なったか」を見るのに元の疎密が要るため。
enum Edge {
  /// 実現可能
  Feasible {
    /// この行に課される demerits（[`PathCost::then`] が積み上げ、DP はその合計を [`PathCost`] として
    /// 最小化する）
    demerits: f64,
    /// 疎密の罰点。`INFINITE_BADNESS` で頭打ち
    badness: f64,
    /// 調整比（正 = 伸長 / 負 = 収縮）。クランプしない生の比で、最終行は常に 0.0
    ratio: f64,
  },
  /// このエッジ単体では組めない（伸縮点が無いのに余る＝孤立した長語など）。行頭を前へずらすと
  /// 空白を得て組めることがあるため早期打ち切りはしない
  Infeasible,
  /// 収縮能力を超えて溢れる（実現不能）。行頭を前へずらすと更に長くなるため早期打ち切りに使う
  Overflow,
}

/// 強制改行を含まない 1 サブ段落を最適分割する
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
        math_penalty: None,
        is_end: false,
      }),
      HItem::Penalty { value } if *value <= 0 => breaks.push(Breakpoint {
        at: i,
        hyphen: false,
        math_penalty: None,
        is_end: false,
      }),
      HItem::Discretionary { .. } => breaks.push(Breakpoint {
        at: i,
        hyphen: true,
        math_penalty: None,
        is_end: false,
      }),
      HItem::MathBreak { penalty, .. } => breaks.push(Breakpoint {
        at: i,
        hyphen: false,
        math_penalty: Some(*penalty),
        is_end: false,
      }),
      // 破断候補にならないアイテム。`Glue` / `Penalty` が上の arm にも出るのは、
      // 分割不可な glue（`breakable: false`）と正の penalty をここで落とすため。
      // `ForcedBreak` は段落を部分段落へ切る側（`break_subparagraph` の呼び出し元）が扱う。
      HItem::Glue { .. }
      | HItem::Penalty { .. }
      | HItem::Box(_)
      | HItem::Kern(_)
      | HItem::FlushRight(_)
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::Footnote(_)
      | HItem::IndexMark { .. } => {},
    }
  }
  breaks.push(Breakpoint {
    at: items.len(),
    hyphen: false,
    math_penalty: None,
    is_end: true,
  });

  // DP: best[j] = 破断点 breaks[j] で行を終える最小経路コスト（到達不能なら None）。
  // prev[j] = Some(i) で直前破断が breaks[i]、None で行頭（item 0）から始まる（best[j] が Some の時のみ有効）。
  // best_badness[j] = 採用したエッジ単体の badness（TRACE 観測用。DP の選択には使わない）。
  let node_count = breaks.len();
  let mut best: Vec<Option<PathCost>> = vec![None; node_count];
  let mut prev: Vec<Option<usize>> = vec![None; node_count];
  let mut best_badness = vec![0.0f64; node_count];

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
        Edge::Feasible {
          demerits, badness, ..
        } => {
          if let Some(from) = best[i] {
            let total = from.then(&breaks[j], demerits);
            if best[j].is_none_or(|current| return total < current) {
              best[j] = Some(total);
              prev[j] = Some(i);
              best_badness[j] = badness;
            }
          }
        },
      }
    }
    // START エッジ（行頭 = item 0 から breaks[j] まで）。より長い行なので溢れ済みなら不要
    if !overflowed
      && let Edge::Feasible {
        demerits, badness, ..
      } = edge_cost(items, 0, &breaks[j], false, text_width)
    {
      let total = PathCost::START.then(&breaks[j], demerits);
      if best[j].is_none_or(|current| return total < current) {
        best[j] = Some(total);
        prev[j] = None;
        best_badness[j] = badness;
      }
    }
  }

  // 末尾の仮想破断点へ到達できない（実現可能な分割が無い）ならフォールバック
  if best[node_count - 1].is_none() {
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
  for (line_index, &node) in chain.iter().enumerate() {
    let brk = &breaks[node];
    let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
    let refs = strip_leading_glue(&refs);
    let trailing_hyphen = match items.get(brk.at) {
      Some(HItem::Discretionary { hyphen }) => Some(hyphen),
      _ => None,
    };
    let line = build_line(refs, brk.is_end, text_width, TextAlignment::Justify, open_links, trailing_hyphen);
    // `line_index` はサブ段落内の連番（強制改行ごとに 0 へ戻る）。段落通し番号ではない
    trace!(
      line_index,
      break_at = brk.at,
      is_last = brk.is_end,
      is_hyphenated = brk.hyphen,
      is_math_break = brk.math_penalty.is_some(),
      badness = best_badness[node],
      width_pt = %line.width().to_pt(),
      text = observe::summarize_line(&line),
      "行を確定"
    );
    lines.push(line);
    line_start = brk.at + 1;
  }
  return lines;
}

/// 候補行のコストを評価し、結果を TRACE へ出す
///
/// 評価そのものは [`evaluate_edge`] が行う。return 点が多いので、観測はこのラッパ 1 箇所に集約する。
fn edge_cost(items: &[HItem], line_start: usize, brk: &Breakpoint, prev_hyphen: bool, text_width: Length) -> Edge {
  let edge = evaluate_edge(items, line_start, brk, prev_hyphen, text_width);
  let (outcome, demerits, badness, ratio) = match &edge {
    Edge::Feasible {
      demerits,
      badness,
      ratio,
    } => ("feasible", Some(*demerits), Some(*badness), Some(*ratio)),
    Edge::Infeasible => ("infeasible", None, None, None),
    Edge::Overflow => ("overflow", None, None, None),
  };
  trace!(
    line_start,
    break_at = brk.at,
    is_last = brk.is_end,
    is_hyphenated = brk.hyphen,
    is_math_break = brk.math_penalty.is_some(),
    is_prev_hyphenated = prev_hyphen,
    outcome,
    demerits = ?demerits,
    badness = ?badness,
    ratio = ?ratio,
    "行分割の候補を評価"
  );
  return edge;
}

/// 候補行（`items[line_start..brk.at]`）の demerits を評価する
fn evaluate_edge(items: &[HItem], line_start: usize, brk: &Breakpoint, prev_hyphen: bool, text_width: Length) -> Edge {
  let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
  let refs = strip_leading_glue(&refs);
  let refs = trim_trailing_glue(refs);

  let (natural, stretch, shrink) = glue_metrics(refs);
  // FlushRight（QED）は右端を占有するため、収まり判定では幅に数える（build_line の自然幅からは除外済み）
  let flush_width: Length = refs
    .iter()
    .filter_map(|item| match item {
      HItem::FlushRight(hbox) => return Some(hbox.width),
      _ => return None,
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
    return Edge::Feasible {
      demerits: demerits(0.0, brk.hyphen, prev_hyphen, brk.math_penalty),
      badness: 0.0,
      ratio: 0.0,
    };
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
  return Edge::Feasible {
    demerits: demerits(badness, brk.hyphen, prev_hyphen, brk.math_penalty),
    badness,
    ratio,
  };
}

/// badness と破断種別から 1 行ぶんの demerits を求める
///
/// 数式内分割点で折った行には penalty の 2 乗を足す（TeX の `\binoppenalty` / `\relpenalty` と同じ形）。
/// 数式内分割点を使うかどうか自体は [`PathCost`] の回数比較で決まるので、この項は数式内分割点どうしの
/// 比較にだけ効く。
fn demerits(badness: f64, hyphen: bool, prev_hyphen: bool, math_penalty: Option<i32>) -> f64 {
  let mut total = (LINE_PENALTY + badness).powi(2);
  if let Some(penalty) = math_penalty {
    total += f64::from(penalty).powi(2);
  }
  if hyphen {
    total += HYPHEN_DEMERIT;
    if prev_hyphen {
      total += DOUBLE_HYPHEN_DEMERIT;
    }
  }
  return total;
}

#[cfg(test)]
mod tests {
  use super::{GreedyBreaker, KnuthPlassBreaker, LineBreaker, PathCost, break_subparagraph, demerits};
  use crate::{
    length::Length,
    style::TextAlignment,
    typeset::{
      boxes::{HItem, Line},
      breaking::break_lines::test_support::{
        box_width, discretionary, flush_right_box, link_target, math_break, stretch_glue, test_box,
      },
    },
  };

  /// 行の右端（box 群の最大右端）
  fn right_edge(line: &Line) -> Length {
    return line.boxes.iter().map(|b| return b.x + b.width).fold(Length::ZERO, Length::max);
  }

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に（sp 丸め精度内で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 2 つの `Length` が（sp 丸め精度内で）一致するか
  fn close_l(a: Length, b: Length) -> bool { return (a - b).abs() <= Length::from_sp(1); }

  #[test]
  fn ragged_right_delegates_to_greedy() {
    // Arrange
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

    // Assert
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
    let lines = KnuthPlassBreaker.break_lines(&[], Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
  }

  #[test]
  fn fits_all_when_width_is_sufficient() {
    let items = vec![test_box(), stretch_glue(), test_box()];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1);
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn justify_flushes_non_final_line_to_right_edge() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      stretch_glue(),
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(right_edge(&lines[0]), 27.0), "非最終行の右端は版面右端: {lines:?}");
  }

  #[test]
  fn uniform_density_beats_greedy_loose_lines() {
    // Arrange
    let glue = || {
      return HItem::Glue {
        natural: pt(2.0),
        stretch: pt(1.0),
        shrink: pt(2.0),
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

    // Act
    let greedy = GreedyBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);
    let kp = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert
    assert_eq!(greedy.len(), 3, "greedy: {greedy:?}");
    assert!(right_edge(&greedy[0]) < pt(24.0), "greedy の非最終行は疎（右端 < 25）: {greedy:?}");

    assert_eq!(kp.len(), 2, "kp: {kp:?}");
    assert!(close(right_edge(&kp[0]), 25.0), "KP の非最終行は右端に揃う: {kp:?}");
  }

  #[test]
  fn forced_break_line_is_not_stretched() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert — 強制改行の直前の行は両端揃えでも伸ばさない（glue は自然幅 5 のまま）
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].x, 15.0), "{lines:?}");
  }

  #[test]
  fn empty_segment_before_forced_break_yields_empty_line() {
    let items = vec![HItem::ForcedBreak, test_box()];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].boxes.is_empty(), "1 行目は空: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn trailing_forced_break_does_not_add_empty_line() {
    let items = vec![test_box(), HItem::ForcedBreak];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(100.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
  }

  #[test]
  fn flush_right_box_wraps_when_it_does_not_fit() {
    // Arrange
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(14.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].x, 6.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    let items = vec![test_box(), stretch_glue(), flush_right_box(8.0)];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(50.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED: {lines:?}");
    assert!(close(lines[0].boxes[1].x, 42.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn link_rect_follows_stretched_glue() {
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
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(27.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!(close(lines[0].links[0].x1, 27.0), "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn inline_atom_box_is_never_split() {
    let items = vec![box_width(50.0)];

    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
    assert!(close(lines[0].boxes[0].width, 50.0));
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    // Arrange
    let items = vec![box_width(20.0), discretionary(3.0), box_width(20.0)];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box + 行末ハイフン: {lines:?}");
    assert!(close(lines[0].boxes[1].width, 3.0), "行末ハイフン: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn avoids_hyphen_break_when_space_break_available() {
    // Arrange
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      discretionary(3.0),
      test_box(),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(25.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    let has_hyphen = lines[0].boxes.len() > 1 && close(lines[0].boxes.last().unwrap().width, 3.0);
    assert!(!has_hyphen, "不要なハイフンを避ける: {lines:?}");
  }

  #[test]
  fn no_feasible_path_falls_back_to_greedy() {
    // Arrange
    let items = vec![
      box_width(40.0),
      HItem::Penalty { value: 0 },
      box_width(40.0),
    ];

    // Act
    let kp = KnuthPlassBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);
    let greedy = GreedyBreaker.break_lines(&items, Length::pt(30.0), TextAlignment::Justify);

    // Assert
    assert_eq!(kp.len(), greedy.len(), "kp: {kp:?}, greedy: {greedy:?}");
    for (kp_line, greedy_line) in kp.iter().zip(&greedy) {
      assert_eq!(kp_line.boxes.len(), greedy_line.boxes.len(), "kp: {kp:?}, greedy: {greedy:?}");
    }
  }

  #[test]
  fn break_subparagraph_stretches_all_but_last_line() {
    // Arrange — [b10 glue b10] が 2 行（自然幅 25・伸長 2.5）。3 箱は最小幅 36.7 > 27 で 1 行に入らず、
    // 1 箱だけの行は glue が無く伸ばせないので、分割は glue 2 本目の 1 通りに決まる
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
    ];

    // Act
    let mut open_links = Vec::new();
    let lines = break_subparagraph(&items, Length::pt(27.0), &mut open_links);

    // Assert — 非最終行は版面右端まで伸び、最終行は自然幅のまま
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(right_edge(&lines[0]), 27.0), "非最終行は右端に揃う: {lines:?}");
    assert!(close(right_edge(&lines[1]), 25.0), "最終行は伸ばさない: {lines:?}");
  }

  #[test]
  fn path_cost_prefers_fewer_math_breaks_over_lower_demerits() {
    let one_math_break = PathCost {
      math_breaks: 1,
      demerits: 0.0,
    };
    let loose_without_math_break = PathCost {
      math_breaks: 0,
      demerits: 1.0e12,
    };
    assert!(loose_without_math_break < one_math_break, "数式内分割の回数を demerits より優先して比べる");
  }

  #[test]
  fn demerits_prefers_relation_break_over_binary_operator_break() {
    assert!(
      demerits(0.0, false, false, Some(500)) < demerits(0.0, false, false, Some(700)),
      "penalty の 2 乗が効くので、関係子の分割点（500）が二項演算子の分割点（700）より優先される"
    );
  }

  #[test]
  fn demerits_prefers_no_math_break_over_a_math_break() {
    assert!(
      demerits(0.0, false, false, None) < demerits(0.0, false, false, Some(500)),
      "数式内分割点を使わない行の方が demerits が低い"
    );
  }

  #[test]
  fn uses_math_break_when_no_other_fit_exists() {
    // Arrange — [b20][glue][b10][MB 3][b20] を幅 36 に。空白で折ると 1 行目が伸縮点の無い b20 だけ
    // （実現不能）、折らないと 58 で溢れる。数式内分割点で折る道だけが残る
    let items = vec![
      box_width(20.0),
      stretch_glue(),
      box_width(10.0),
      math_break(3.0, 500),
      box_width(20.0),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(36.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "{lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "{lines:?}");
    assert_eq!(lines[1].boxes[0].x, Length::ZERO, "演算子後のアキは次行の行頭に残らない: {lines:?}");
  }

  #[test]
  fn prefers_loose_line_over_math_break() {
    // Arrange — 幅 40。数式内分割点で折れば 1 行目がぴったり（badness 0）だが、空白で折る疎な行
    // （badness 上限）が実現可能なので、そちらを選ぶ
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      math_break(5.0, 1),
      box_width(10.0),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(40.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "2 つ目の空白で折る: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
    assert_eq!(lines[1].boxes[1].x, Length::pt(15.0), "折らなかった分割点はアキとして残る: {lines:?}");
  }

  #[test]
  fn unbroken_math_break_counts_toward_justify_ratio_on_non_final_line() {
    // Arrange — [b10][glue][b5][MB 5][b5][Penalty0][b10] を幅 32 に。
    // 分割点は glue（index1）と penalty（index5）の 2 つだけ。全体は自然幅 40 で単独行には収まらない
    // ので、必ず penalty で 2 行に折る。1 行目 [b10, glue, b5, MB, b5] は非最終行で MB を折らずに含む。
    // MB のアキ（5）を自然幅に数えなければ両端揃えの配分比がずれて右端が 32 に一致しなくなる。
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(5.0),
      math_break(5.0, 1),
      box_width(5.0),
      HItem::Penalty { value: 0 },
      box_width(10.0),
    ];

    // Act
    let lines = KnuthPlassBreaker.break_lines(&items, Length::pt(32.0), TextAlignment::Justify);

    // Assert
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 3, "本文 box 3 つ（MB はボックスを生成しない）: {lines:?}");
    assert!(
      close(right_edge(&lines[0]), 32.0),
      "MB のアキを自然幅に数えて配分比を計算するので右端は版面幅に一致: {lines:?}"
    );
  }
}
