//! Knuth–Plass 方式の段落全体最適な行分割

use tracing::trace;

use crate::{
  document::TextAlignment,
  length::{Length, LengthOverflow},
  typeset::{
    boxes::{HItem, Line},
    boxing::break_lines::{
      LineEnd, OpenLink, build_line, glue_metrics, greedy, strip_leading_glue, trailing_comma, trim_trailing_glue,
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

/// Knuth–Plass 方式（段落全体最適）で水平リストを本文幅の両端揃えの行に分割する
///
/// 行末の句読点（[`HItem::Comma`]）は、その幅が `hang_room` 以下で、ぶら下げると行が実現可能になるか
/// より安くなるときだけ利用幅の外へぶら下げる。
///
/// # Errors
///
/// 行の幅や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
pub(super) fn break_lines(items: &[HItem], text_width: Length, hang_room: Length) -> Result<Vec<Line>, LengthOverflow> {
  // 強制改行（ForcedBreak）で区切ったサブ段落ごとに最適化する。各サブ段落の最終行は伸縮しない
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
    let segment_lines = break_subparagraph(segment, text_width, hang_room, &mut open_links)?;
    lines.extend(segment_lines);
  }
  return Ok(lines);
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
/// 伸縮点の無い非最終行の本数 → 数式内分割点（[`HItem::MathBreak`]）の使用回数 → demerits の順に比べる
/// （derive した `PartialOrd` はフィールドの宣言順に辞書式比較するので、フィールド順が優先順位になる）。
/// 伸縮点の無い行を含む経路は、含まない経路に demerits や数式内分割の回数によらず常に負ける。含まない経路
/// どうしでは `rigid_lines` が 0 で揃い、数式内分割点を含まない段落では `math_breaks` も 0 で揃うので、
/// 比較は後ろの項だけのものと一致する。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
struct PathCost {
  /// 経路上の伸縮点の無い非最終行の本数
  rigid_lines: u32,
  /// 経路上の数式内分割の回数
  math_breaks: u32,
  /// 経路上の demerits の総和
  demerits: f64,
}

impl PathCost {
  /// 経路の起点（行頭 = item 0）のコスト
  const START: PathCost = PathCost {
    rigid_lines: 0,
    math_breaks: 0,
    demerits: 0.0,
  };

  /// 破断点 `brk` で終わる 1 行（demerits `demerits`、伸縮点の無い非最終行なら `rigid`）を継ぎ足した
  /// 経路のコストを返す
  fn then(self, brk: &Breakpoint, demerits: f64, rigid: bool) -> Self {
    return PathCost {
      rigid_lines: self.rigid_lines + u32::from(rigid),
      math_breaks: self.math_breaks + u32::from(brk.math_penalty.is_some()),
      demerits: self.demerits + demerits,
    };
  }
}

/// 1 本の候補行（開始位置 → 破断点）のコスト評価結果
enum Edge {
  /// 実現可能
  Feasible {
    /// この行に課される demerits
    demerits: f64,
    /// 疎密の罰点。`INFINITE_BADNESS` で頭打ち
    badness: f64,
    /// 調整比（正 = 伸長 / 負 = 収縮）。クランプしない生の比で、最終行と伸縮点の無い行は常に 0.0
    ratio: f64,
    /// 伸縮点が無いのに余る非最終行（数式だけの行・孤立した長語など）か。`build_line` が伸縮せず自然幅で
    /// 左に寄せるので右端は揃わない
    rigid: bool,
    /// 行末の句読点をぶら下げて組む案か
    hanging: bool,
  },
  /// ぶら下げる案を含めて収縮能力を超えて溢れる（実現不能）。行頭を前へずらすと更に長くなるため早期打ち切りに使う
  Overflow,
}

/// 強制改行を含まない 1 サブ段落を最適分割する。溢れる行を避けられない（末尾まで実現可能な分割が無い）サブ段落だけを
/// 貪欲法（`greedy::break_lines`）で組み、開いているリンクも貪欲法へ引き継ぐ
fn break_subparagraph(
  items: &[HItem],
  text_width: Length,
  hang_room: Length,
  open_links: &mut Vec<OpenLink>,
) -> Result<Vec<Line>, LengthOverflow> {
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
      // 破断候補にならないアイテム（分割不可な glue・正の penalty を含む）
      HItem::Glue { .. }
      | HItem::Penalty { .. }
      | HItem::Box(_)
      | HItem::Comma(_)
      | HItem::Kern(_)
      | HItem::FlushRight(_)
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::Footnote(_)
      | HItem::IndexMark(_) => {},
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
  // best_hanging[j] = 採用したエッジが行末の句読点をぶら下げる案か。
  let node_count = breaks.len();
  let mut best: Vec<Option<PathCost>> = vec![None; node_count];
  let mut prev: Vec<Option<usize>> = vec![None; node_count];
  let mut best_badness = vec![0.0f64; node_count];
  let mut best_hanging = vec![false; node_count];

  for j in 0..node_count {
    // 行頭候補を近い側（短い行）から見て、溢れたら以降（より長い行）は不要なので打ち切る
    let mut overflowed = false;
    for i in (0..j).rev() {
      let line_start = breaks[i].at + 1;
      match edge_cost(items, line_start, &breaks[j], breaks[i].hyphen, text_width, hang_room)? {
        Edge::Overflow => {
          overflowed = true;
          break;
        },
        Edge::Feasible {
          demerits,
          badness,
          rigid,
          hanging,
          ..
        } => {
          if let Some(from) = best[i] {
            let total = from.then(&breaks[j], demerits, rigid);
            if best[j].is_none_or(|current| return total < current) {
              best[j] = Some(total);
              prev[j] = Some(i);
              best_badness[j] = badness;
              best_hanging[j] = hanging;
            }
          }
        },
      }
    }
    // START エッジ（行頭 = item 0 から breaks[j] まで）。より長い行なので溢れ済みなら不要
    if !overflowed
      && let Edge::Feasible {
        demerits,
        badness,
        rigid,
        hanging,
        ..
      } = edge_cost(items, 0, &breaks[j], false, text_width, hang_room)?
    {
      let total = PathCost::START.then(&breaks[j], demerits, rigid);
      if best[j].is_none_or(|current| return total < current) {
        best[j] = Some(total);
        prev[j] = None;
        best_badness[j] = badness;
        best_hanging[j] = hanging;
      }
    }
  }

  // 末尾の仮想破断点へ到達できない（実現可能な分割が無い）ならフォールバック
  if best[node_count - 1].is_none() {
    return greedy::break_lines(items, text_width, TextAlignment::Justify, open_links);
  }

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

  let mut lines: Vec<Line> = Vec::new();
  let mut line_start = 0usize;
  for (line_index, &node) in chain.iter().enumerate() {
    let brk = &breaks[node];
    let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
    let refs = strip_leading_glue(&refs);
    let end = if best_hanging[node] {
      LineEnd::Hang(trailing_comma(refs).expect("ぶら下げ案は行末が句読点の候補行にだけ作る"))
    } else {
      match items.get(brk.at) {
        Some(HItem::Discretionary { hyphen }) => LineEnd::Hyphen(hyphen),
        _ => LineEnd::Plain,
      }
    };
    let line = build_line(refs, brk.is_end, text_width, TextAlignment::Justify, open_links, end)?;
    let width = line.width()?;
    // `line_index` はサブ段落内の連番（強制改行ごとに 0 へ戻る）。段落通し番号ではない
    trace!(
      line_index,
      break_at = brk.at,
      is_last = brk.is_end,
      is_hyphenated = brk.hyphen,
      is_math_break = brk.math_penalty.is_some(),
      is_hanging = best_hanging[node],
      badness = best_badness[node],
      width_pt = %width.to_pt(),
      text = observe::summarize_line(&line),
      "行を確定"
    );
    lines.push(line);
    line_start = brk.at + 1;
  }
  return Ok(lines);
}

/// 候補行のコストを評価し、結果を TRACE へ出す
fn edge_cost(
  items: &[HItem],
  line_start: usize,
  brk: &Breakpoint,
  prev_hyphen: bool,
  text_width: Length,
  hang_room: Length,
) -> Result<Edge, LengthOverflow> {
  let edge = evaluate_edge(items, line_start, brk, prev_hyphen, text_width, hang_room)?;
  let (outcome, demerits, badness, ratio, is_hanging) = match &edge {
    Edge::Feasible {
      demerits,
      badness,
      ratio,
      rigid,
      hanging,
    } => (if *rigid { "rigid" } else { "feasible" }, Some(*demerits), Some(*badness), Some(*ratio), *hanging),
    Edge::Overflow => ("overflow", None, None, None, false),
  };
  trace!(
    line_start,
    break_at = brk.at,
    is_last = brk.is_end,
    is_hyphenated = brk.hyphen,
    is_math_break = brk.math_penalty.is_some(),
    is_prev_hyphenated = prev_hyphen,
    is_hanging,
    outcome,
    demerits = ?demerits,
    badness = ?badness,
    ratio = ?ratio,
    "行分割の候補を評価"
  );
  return Ok(edge);
}

/// 候補行（`items[line_start..brk.at]`）の demerits を評価する
///
/// 自然幅が利用幅を溢れ、行末が幅 `hang_room` 以下の句読点で語中破断でない行は、句読点をぶら下げる案も評価する。
/// 両案とも実現可能なら伸縮点の無い行か → demerits の順に小さい方（同値はぶら下げない案）を採る。
fn evaluate_edge(
  items: &[HItem],
  line_start: usize,
  brk: &Breakpoint,
  prev_hyphen: bool,
  text_width: Length,
  hang_room: Length,
) -> Result<Edge, LengthOverflow> {
  let refs: Vec<&HItem> = items[line_start..brk.at].iter().collect();
  let refs = strip_leading_glue(&refs);
  let refs = trim_trailing_glue(refs);

  let (natural, stretch, shrink) = glue_metrics(refs)?;
  // FlushRight（QED）は右端を占有するため、収まり判定では幅に数える（`glue_metrics` の自然幅には含まれない）
  let flush_width = Length::try_sum(refs.iter().filter_map(|item| match item {
    HItem::FlushRight(hbox) => return Some(hbox.width),
    _ => return None,
  }))?;
  // 語中破断は行末ハイフンぶん本文幅を狭める
  let hyphen_width = match items.get(brk.at) {
    Some(HItem::Discretionary { hyphen }) => hyphen.width,
    _ => Length::ZERO,
  };
  let available = text_width.try_sub(hyphen_width)?;
  let leftover = available.try_sub(natural.try_add(flush_width)?)?;
  let plain = fit_edge(leftover, stretch, shrink, false, brk, prev_hyphen);

  // ぶら下げは自然幅で溢れる行だけの候補。語中破断の行末はハイフンなので句読点で終わらない
  let hang_width = match trailing_comma(refs) {
    Some(comma) if leftover < Length::ZERO && !brk.hyphen && comma.width <= hang_room => comma.width,
    _ => return Ok(plain),
  };
  let hung = fit_edge(leftover.try_add(hang_width)?, stretch, shrink, true, brk, prev_hyphen);
  return Ok(match (plain, hung) {
    (Edge::Overflow, edge) | (edge, Edge::Overflow) => edge,
    (
      plain @ Edge::Feasible {
        demerits: plain_demerits,
        rigid: plain_rigid,
        ..
      },
      hung @ Edge::Feasible {
        demerits: hung_demerits,
        rigid: hung_rigid,
        ..
      },
    ) => {
      if (hung_rigid, hung_demerits) < (plain_rigid, plain_demerits) {
        hung
      } else {
        plain
      }
    },
  });
}

/// 利用幅に対する余り幅 `leftover`（負なら溢れ）と行の伸縮能力から、候補行の収まりと demerits を求める
fn fit_edge(
  leftover: Length,
  stretch: Length,
  shrink: Length,
  hanging: bool,
  brk: &Breakpoint,
  prev_hyphen: bool,
) -> Edge {
  // 最終行は build_line が伸縮なしで組むため、自然幅で収まらなければ実現不能。
  // 収まってさえいれば疎密を罰しない（badness 0）
  if brk.is_end {
    if leftover < Length::ZERO {
      return Edge::Overflow;
    }
    return Edge::Feasible {
      demerits: demerits(0.0, brk.hyphen, prev_hyphen, brk.math_penalty),
      badness: 0.0,
      ratio: 0.0,
      rigid: false,
      hanging,
    };
  }

  // 非最終行は収縮を使える。収縮能力を超えて溢れる行は実現不能
  let overflows = leftover < Length::ZERO && (!shrink.is_positive() || leftover.ratio(shrink) < -1.0);
  if overflows {
    return Edge::Overflow;
  }

  // 非最終行の調整比。伸縮点が無いのに余る行は両端揃えできないが、伸縮せず自然幅で組める最下位の行として
  // 候補に残す（選ぶかどうかは `PathCost` の `rigid_lines` が決めるので、badness は上限に置くだけ）
  let ratio: f64 = match leftover.cmp(&Length::ZERO) {
    std::cmp::Ordering::Greater => {
      if !stretch.is_positive() {
        return Edge::Feasible {
          demerits: demerits(INFINITE_BADNESS, brk.hyphen, prev_hyphen, brk.math_penalty),
          badness: INFINITE_BADNESS,
          ratio: 0.0,
          rigid: true,
          hanging,
        };
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
    rigid: false,
    hanging,
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
  use super::{Breakpoint, Edge, PathCost, break_lines, break_subparagraph, demerits, evaluate_edge};
  use crate::{
    document::TextAlignment,
    length::Length,
    typeset::{
      boxes::{HItem, Line},
      boxing::break_lines::{
        greedy,
        test_support::{
          box_width, cjk_glue, comma, comma_aki, discretionary, flush_right_box, hang_items, link_target, math_break,
          stretch_glue, test_box,
        },
      },
    },
  };

  /// 行の右端（box 群の最大右端）
  fn right_edge(line: &Line) -> Length {
    return line.boxes.iter().map(|b| return b.dx + b.hbox.width).fold(Length::ZERO, Length::max);
  }

  /// 行の box 幅を左から並べたもの（行末ハイフンを含む）
  fn box_widths(line: &Line) -> Vec<Length> { return line.boxes.iter().map(|b| return b.hbox.width).collect(); }

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に（差 1e-3 pt 未満で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 各行のリンク矩形が `expected`（行ごとの `(x0, x1)` の pt 値）と一致し、行き先がすべて `link_target()` か
  fn assert_link_spans(lines: &[Line], expected: &[&[(f32, f32)]]) {
    assert_eq!(lines.len(), expected.len(), "{lines:?}");
    for (line, spans) in lines.iter().zip(expected) {
      assert_eq!(line.links.len(), spans.len(), "{lines:?}");
      for (link, &(x0, x1)) in line.links.iter().zip(*spans) {
        assert!(close(link.x0, x0) && close(link.x1, x1), "期待 ({x0}, {x1}): {lines:?}");
        assert_eq!(link.target, link_target(), "{lines:?}");
      }
    }
  }

  #[test]
  fn empty_items_yield_single_empty_line() {
    let lines = break_lines(&[], Length::pt(100.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 1);
    assert!(lines[0].boxes.is_empty());
  }

  #[test]
  fn uniform_density_beats_greedy_loose_lines() {
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

    let greedy = greedy::break_lines(&items, Length::pt(25.0), TextAlignment::Justify, &mut Vec::new())
      .expect("テストの寸法は値域内");
    let kp = break_lines(&items, Length::pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(greedy.len(), 3, "greedy: {greedy:?}");
    assert!(right_edge(&greedy[0]) < pt(24.0), "greedy の非最終行は疎（右端 < 25）: {greedy:?}");

    assert_eq!(kp.len(), 2, "kp: {kp:?}");
    assert!(close(right_edge(&kp[0]), 25.0), "KP の非最終行は右端に揃う: {kp:?}");
  }

  #[test]
  fn forced_break_line_is_not_stretched() {
    let items = vec![
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::ForcedBreak,
      test_box(),
    ];

    let lines = break_lines(&items, Length::pt(27.0), Length::ZERO).expect("テストの寸法は値域内");

    // glue は自然幅 5 のまま
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn empty_segment_before_forced_break_yields_empty_line() {
    let items = vec![HItem::ForcedBreak, test_box()];

    let lines = break_lines(&items, Length::pt(100.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].boxes.is_empty(), "1 行目は空: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn trailing_forced_break_does_not_add_empty_line() {
    let items = vec![test_box(), HItem::ForcedBreak];

    let lines = break_lines(&items, Length::pt(100.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1);
  }

  #[test]
  fn flush_right_box_wraps_when_it_does_not_fit() {
    let items = vec![
      test_box(),
      HItem::Penalty { value: 0 },
      flush_right_box(8.0),
    ];

    let lines = break_lines(&items, Length::pt(14.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].dx, 6.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    let items = vec![test_box(), stretch_glue(), flush_right_box(8.0)];

    let lines = break_lines(&items, Length::pt(50.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "本文 box と QED: {lines:?}");
    assert!(close(lines[0].boxes[1].dx, 42.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn link_continues_into_a_greedy_fallback_subparagraph() {
    // 2 つ目のサブ段落は本文幅を超える b40 を含み、実現可能な分割が無いので貪欲法へ落ちる
    let items = vec![
      HItem::LinkStart(link_target()),
      box_width(10.0),
      HItem::ForcedBreak,
      box_width(40.0),
      stretch_glue(),
      box_width(10.0),
      HItem::LinkEnd,
    ];

    let lines = break_lines(&items, pt(30.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_link_spans(&lines, &[&[(0.0, 10.0)], &[(0.0, 40.0)], &[(0.0, 10.0)]]);
  }

  #[test]
  fn link_opened_in_a_greedy_fallback_subparagraph_continues_after_it() {
    // 1 つ目のサブ段落は b40 を含んで貪欲法へ落ち、その中で開いたリンクが次のサブ段落で閉じる
    let items = vec![
      box_width(40.0),
      stretch_glue(),
      HItem::LinkStart(link_target()),
      box_width(10.0),
      HItem::ForcedBreak,
      box_width(10.0),
      HItem::LinkEnd,
    ];

    let lines = break_lines(&items, pt(30.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_link_spans(&lines, &[&[], &[(0.0, 10.0)], &[(0.0, 10.0)]]);
  }

  #[test]
  fn link_continues_across_consecutive_greedy_fallback_subparagraphs() {
    // どちらのサブ段落も b40 だけで溢れて貪欲法へ落ちる
    let items = vec![
      HItem::LinkStart(link_target()),
      box_width(40.0),
      HItem::ForcedBreak,
      box_width(40.0),
      HItem::LinkEnd,
    ];

    let lines = break_lines(&items, pt(30.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_link_spans(&lines, &[&[(0.0, 40.0)], &[(0.0, 40.0)]]);
  }

  #[test]
  fn link_across_forced_break_without_fallback_has_a_rect_per_line() {
    let items = vec![
      HItem::LinkStart(link_target()),
      box_width(10.0),
      HItem::ForcedBreak,
      box_width(10.0),
      HItem::LinkEnd,
    ];

    let lines = break_lines(&items, pt(30.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_link_spans(&lines, &[&[(0.0, 10.0)], &[(0.0, 10.0)]]);
  }

  #[test]
  fn link_rect_follows_stretched_glue() {
    let items = vec![
      HItem::LinkStart(link_target()),
      test_box(),
      stretch_glue(),
      test_box(),
      HItem::LinkEnd,
      stretch_glue(),
      test_box(),
    ];

    let lines = break_lines(&items, Length::pt(27.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].links.len(), 1, "{:?}", lines[0].links);
    assert!(close(lines[0].links[0].x1, 27.0), "リンク矩形は伸縮後の字位置: {:?}", lines[0].links);
  }

  #[test]
  fn breaks_at_discretionary_and_appends_hyphen() {
    // 幅 25。1 本目の空白で折ると 1 行目が伸縮点の無い b10 だけの行になり、2 本目の空白で折ると
    // b10+5+b7+b10 = 32 が収縮 5/3 を超えて溢れる。語中分割で折る 1 行目は b10+5+b7 = 22 が
    // 本文幅からハイフン 3 を引いた 22 にぴったり収まり、2 行目 b10+5+b10 = 25 も最終行に収まる。
    // 伸縮点の無い行を含まない経路は語中分割で折る 1 通りだけなので、それが選ばれる
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(7.0),
      discretionary(3.0),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, Length::pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(box_widths(&lines[0]), vec![pt(10.0), pt(7.0), pt(3.0)], "行末にハイフンが付く: {lines:?}");
    assert_eq!(lines[0].boxes[2].dx, pt(22.0), "ハイフンは行内アイテムの直後に置く: {lines:?}");
    assert_eq!(box_widths(&lines[1]), vec![pt(10.0), pt(10.0)], "{lines:?}");
  }

  #[test]
  fn avoids_hyphen_break_when_space_break_available() {
    // 幅 40。語中分割で折る 1 行目 b10+5+b10+5+b6 = 36 は、本文幅からハイフン 3 を引いた 37 へ
    // 伸長 5 のうち 1 を使う（比 0.2・badness 0.8）ので demerits は 10.8² + 2500 = 2616.64。
    // 3 本目の空白で折る 1 行目 b10+5+b10+5+b6+b6 = 42 は収縮 10/3 のうち 2 を使う
    // （比 -0.6・badness 21.6）ので 31.6² = 998.56。2 本目の空白は 1 行目 25 が伸長 2.5 で 15 余り、
    // 1 本目は b10 だけの伸縮点の無い行になるので選ばれない。疎密ではハイフン行が勝つが、ハイフンの demerits 2500 で逆転する
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(6.0),
      discretionary(3.0),
      box_width(6.0),
      stretch_glue(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, Length::pt(40.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
      box_widths(&lines[0]),
      vec![pt(10.0), pt(10.0), pt(6.0), pt(6.0)],
      "語中分割でなく空白で折る: {lines:?}"
    );
    assert_eq!(box_widths(&lines[1]), vec![pt(10.0)], "{lines:?}");
  }

  #[test]
  fn avoids_consecutive_hyphen_breaks() {
    // 幅 25。1 行目は `breaks_at_discretionary_and_appends_hyphen` と同じく語中分割でしか組めない。
    // 2 行目を 2 つ目の語中分割で折ると b14+5+b2 = 21 が 22 へ伸長 2.5 のうち 1 を使う
    // （比 0.4・badness 6.4）ので 16.4² + 2500 = 2768.96。3 本目の空白で折ると b14+5+b2+b2 = 23 が
    // 25 へ 2 を使う（比 0.8・badness 51.2）ので 61.2² = 3745.44。2 本目の空白は b14 だけの伸縮点の無い行になるので選ばれず、
    // 折らずに最終行とすると 38 で溢れる。単独ではハイフン行が安いが、前の行もハイフンで終わるので
    // 連続ハイフンの demerits 10000 が加わって空白で折る側が勝つ
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(7.0),
      discretionary(3.0),
      box_width(14.0),
      stretch_glue(),
      box_width(2.0),
      discretionary(3.0),
      box_width(2.0),
      stretch_glue(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, Length::pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(box_widths(&lines[0]), vec![pt(10.0), pt(7.0), pt(3.0)], "{lines:?}");
    assert_eq!(box_widths(&lines[1]), vec![pt(14.0), pt(2.0), pt(2.0)], "ハイフンを続けず空白で折る: {lines:?}");
    assert_eq!(box_widths(&lines[2]), vec![pt(10.0)], "{lines:?}");
  }

  #[test]
  fn rejects_hyphen_break_that_overflows_by_hyphen_width() {
    // 幅 34。語中分割で折る 1 行目 b10+5+b10+5+b5 = 35 は、ハイフン 3 を引いた 31 に対して 4 溢れ、
    // 収縮 10/3 を超えるので実現不能（ハイフン幅を数えなければ 1 溢れるだけで badness 2.7 の行になる）。
    // 3 本目の空白で折る 1 行目 b10+5+b10+5+b5+b2 = 37 は 34 へ収縮 3 を使う（比 -0.9）ので組める。
    // 2 本目の空白は 1 行目 25 が伸長 2.5 で 9 余り、1 本目は b10 だけの伸縮点の無い行になるので選ばれない
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(5.0),
      discretionary(3.0),
      box_width(2.0),
      stretch_glue(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, Length::pt(34.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
      box_widths(&lines[0]),
      vec![pt(10.0), pt(10.0), pt(5.0), pt(2.0)],
      "ハイフン込みで溢れる語中分割は選ばない: {lines:?}"
    );
    assert_eq!(box_widths(&lines[1]), vec![pt(10.0)], "{lines:?}");
  }

  #[test]
  fn no_feasible_path_falls_back_to_greedy() {
    let items = vec![
      box_width(40.0),
      HItem::Penalty { value: 0 },
      box_width(40.0),
    ];

    let kp = break_lines(&items, Length::pt(30.0), Length::ZERO).expect("テストの寸法は値域内");
    let greedy = greedy::break_lines(&items, Length::pt(30.0), TextAlignment::Justify, &mut Vec::new())
      .expect("テストの寸法は値域内");

    assert_eq!(kp.len(), greedy.len(), "kp: {kp:?}, greedy: {greedy:?}");
    for (kp_line, greedy_line) in kp.iter().zip(&greedy) {
      assert_eq!(kp_line.boxes.len(), greedy_line.boxes.len(), "kp: {kp:?}, greedy: {greedy:?}");
    }
  }

  #[test]
  fn break_subparagraph_stretches_all_but_last_line() {
    // [b10 glue b10] が 2 行（自然幅 25・伸長 2.5）。3 箱は最小幅 36.7 > 27 で 1 行に入らず、
    // 1 箱だけの行は glue が無く伸縮点の無い行になるので、選ばれる分割は glue 2 本目の 1 通りに決まる
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
    ];

    let mut open_links = Vec::new();
    let lines =
      break_subparagraph(&items, Length::pt(27.0), Length::ZERO, &mut open_links).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(right_edge(&lines[0]), 27.0), "非最終行は右端に揃う: {lines:?}");
    assert!(close(right_edge(&lines[1]), 25.0), "最終行は伸ばさない: {lines:?}");
  }

  #[test]
  fn path_cost_prefers_fewer_math_breaks_over_lower_demerits() {
    let one_math_break = PathCost {
      rigid_lines: 0,
      math_breaks: 1,
      demerits: 0.0,
    };
    let loose_without_math_break = PathCost {
      rigid_lines: 0,
      math_breaks: 0,
      demerits: 1.0e12,
    };
    assert!(loose_without_math_break < one_math_break, "数式内分割の回数を demerits より優先して比べる");
  }

  #[test]
  fn path_cost_prefers_fewer_rigid_lines_over_fewer_math_breaks() {
    let one_rigid_line = PathCost {
      rigid_lines: 1,
      math_breaks: 0,
      demerits: 0.0,
    };
    let math_breaks_without_rigid_line = PathCost {
      rigid_lines: 0,
      math_breaks: 5,
      demerits: 1.0e12,
    };
    assert!(
      math_breaks_without_rigid_line < one_rigid_line,
      "伸縮点の無い行の本数を数式内分割の回数と demerits より優先して比べる"
    );
  }

  #[test]
  fn rigid_first_line_keeps_rest_of_paragraph_optimal() {
    // 幅 25。b22 は次の語と並べると 32（収縮 2 で 30）で溢れるので単独行になり、伸縮点が無い。
    // 残り 5 語は `uniform_density_beats_greedy_loose_lines` と同じ形で、貪欲法は 3 行・Knuth–Plass は 2 行
    let glue = || {
      return HItem::Glue {
        natural: pt(2.0),
        stretch: pt(1.0),
        shrink: pt(2.0),
        breakable: true,
      };
    };
    let items = vec![
      box_width(22.0),
      glue(),
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

    let kp = break_lines(&items, pt(25.0), Length::ZERO).expect("テストの寸法は値域内");
    let greedy =
      greedy::break_lines(&items, pt(25.0), TextAlignment::Justify, &mut Vec::new()).expect("テストの寸法は値域内");

    assert_eq!(greedy.len(), 4, "greedy: {greedy:?}");
    assert_eq!(kp.len(), 3, "kp: {kp:?}");
    assert_eq!(box_widths(&kp[0]), vec![pt(22.0)], "kp: {kp:?}");
    assert_eq!(kp[0].boxes[0].dx, Length::ZERO, "伸縮点の無い行は左に寄せる: {kp:?}");
    assert!(close(right_edge(&kp[0]), 22.0), "伸縮点の無い行は自然幅のまま: {kp:?}");
    assert_eq!(box_widths(&kp[1]), vec![pt(8.0), pt(8.0), pt(8.0)], "kp: {kp:?}");
    assert!(close(right_edge(&kp[1]), 25.0), "後続の非最終行は両端揃え: {kp:?}");
    assert_eq!(box_widths(&kp[2]), vec![pt(8.0), pt(8.0)], "kp: {kp:?}");
  }

  #[test]
  fn math_only_middle_line_keeps_paragraph_optimal() {
    // 幅 25。数式 b12 / b20 / b6 を数式内分割点（アキ 2・penalty 700）でつなぐ。b20 を含む行は
    // 前の b12 とも後ろの b6 とも並べると溢れるので数式だけの行（伸縮点なし）になる。1 行目は
    // 空白で折ると b10 だけの伸縮点の無い行が増えるので、数式内分割点で [b10 glue b12]（24 → 25）に折る
    let glue = || {
      return HItem::Glue {
        natural: pt(2.0),
        stretch: pt(1.0),
        shrink: pt(2.0),
        breakable: true,
      };
    };
    let items = vec![
      box_width(10.0),
      glue(),
      box_width(12.0),
      math_break(2.0, 700),
      box_width(20.0),
      math_break(2.0, 700),
      box_width(6.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
      glue(),
      box_width(8.0),
    ];

    let lines = break_lines(&items, pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(box_widths(&lines[0]), vec![pt(10.0), pt(12.0)], "{lines:?}");
    assert!(close(right_edge(&lines[0]), 25.0), "{lines:?}");
    assert_eq!(box_widths(&lines[1]), vec![pt(20.0)], "{lines:?}");
    assert_eq!(lines[1].boxes[0].dx, Length::ZERO, "{lines:?}");
    assert!(close(right_edge(&lines[1]), 20.0), "数式だけの行は自然幅で左に寄せる: {lines:?}");
    assert_eq!(box_widths(&lines[2]), vec![pt(6.0), pt(8.0), pt(8.0)], "{lines:?}");
    assert!(close(right_edge(&lines[2]), 25.0), "{lines:?}");
    assert_eq!(box_widths(&lines[3]), vec![pt(8.0), pt(8.0)], "{lines:?}");
  }

  #[test]
  fn rigid_line_ending_at_discretionary_places_hyphen_at_natural_edge() {
    // 幅 25。語 b18|b18 は 1 行に収まらず、語中で割った 1 行目 b18 + ハイフン 3 は伸縮点が無い
    let items = vec![box_width(18.0), discretionary(3.0), box_width(18.0)];

    let lines = break_lines(&items, pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(box_widths(&lines[0]), vec![pt(18.0), pt(3.0)], "{lines:?}");
    assert_eq!(lines[0].boxes[1].dx, pt(18.0), "ハイフンは自然幅の右端に付く: {lines:?}");
  }

  #[test]
  fn exact_fit_line_without_stretch_is_not_rigid() {
    // [b25] は伸縮点が無いが幅 25 にぴったり、[b20] は 5 余る
    let items = vec![
      box_width(25.0),
      HItem::Penalty { value: 0 },
      box_width(20.0),
      HItem::Penalty { value: 0 },
      box_width(5.0),
    ];
    let brk = |at| {
      return Breakpoint {
        at,
        hyphen: false,
        math_penalty: None,
        is_end: false,
      };
    };

    let exact = evaluate_edge(&items, 0, &brk(1), false, pt(25.0), Length::ZERO).expect("テストの寸法は値域内");
    let short = evaluate_edge(&items, 2, &brk(3), false, pt(25.0), Length::ZERO).expect("テストの寸法は値域内");

    assert!(
      matches!(exact, Edge::Feasible { rigid: false, badness, .. } if badness == 0.0),
      "ぴったりの行は通常の行"
    );
    assert!(matches!(short, Edge::Feasible { rigid: true, ratio, .. } if ratio == 0.0), "余る行は伸縮点の無い行");
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
    // 空白で折ると 1 行目が伸縮点の無い b20 だけの行になり、折らないと 58 で溢れる。伸縮点の無い行の本数を
    // 数式内分割の回数より先に比べるので、数式内分割点で折る経路が勝つ
    let items = vec![
      box_width(20.0),
      stretch_glue(),
      box_width(10.0),
      math_break(3.0, 500),
      box_width(20.0),
    ];

    let lines = break_lines(&items, Length::pt(36.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "{lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "{lines:?}");
    assert_eq!(lines[1].boxes[0].dx, Length::ZERO, "演算子後のアキは次行の行頭に残らない: {lines:?}");
  }

  #[test]
  fn prefers_loose_line_over_math_break() {
    // 幅 40。数式内分割点で折れば 1 行目がぴったり（badness 0）だが、空白で折る疎な行
    // （badness 上限）も実現可能
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      math_break(5.0, 1),
      box_width(10.0),
    ];

    let lines = break_lines(&items, Length::pt(40.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 2, "2 つ目の空白で折る: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 2, "{lines:?}");
    assert_eq!(lines[1].boxes[1].dx, Length::pt(15.0), "折らなかった分割点はアキとして残る: {lines:?}");
  }

  #[test]
  fn unbroken_math_break_counts_toward_justify_ratio_on_non_final_line() {
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

    let lines = break_lines(&items, Length::pt(32.0), Length::ZERO).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 3, "本文 box 3 つ（MB はボックスを生成しない）: {lines:?}");
    assert!(
      close(right_edge(&lines[0]), 32.0),
      "MB のアキを自然幅に数えて配分比を計算するので右端は版面幅に一致: {lines:?}"
    );
  }

  #[test]
  fn hangs_a_comma_that_alone_overflows() {
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(30.0), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
      lines[0].boxes.last().expect("1 行目は箱を持つ").dx,
      pt(30.0),
      "句読点は利用幅の右端に接する: {lines:?}"
    );
    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
    assert_eq!(lines[0].width_without_hanging().expect("値域内"), pt(30.0), "{lines:?}");
  }

  #[test]
  fn does_not_hang_when_the_room_is_narrower_than_the_comma() {
    let lines = break_lines(&hang_items(), pt(30.0), pt(4.99)).expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
    assert!(lines[0].width().expect("値域内") <= pt(30.0), "{lines:?}");
  }

  #[test]
  fn does_not_hang_a_comma_that_fits_at_natural_width() {
    let lines = break_lines(&hang_items(), pt(35.0), pt(5.0)).expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
  }

  #[test]
  fn hanging_line_stretches_its_body_to_the_full_width() {
    // 句読点を除いた本文 30 を幅 30.5 へ伸ばす（和文字間 glue 2 本の伸長 1 のうち 0.5 を使う）
    let lines = break_lines(&hang_items(), pt(30.5), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines[0].boxes.last().expect("1 行目は箱を持つ").dx, pt(30.5), "{lines:?}");
    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
  }

  #[test]
  fn shrinks_instead_of_hanging_when_that_is_cheaper() {
    // 語間 glue（幅 2・伸長 1・収縮 3）。句読点込みで 39 → 幅 38.5 は収縮比 0.08、ぶら下げは伸長比 2.25
    let glue = || {
      return HItem::Glue {
        natural: pt(2.0),
        stretch: pt(1.0),
        shrink: pt(3.0),
        breakable: true,
      };
    };
    let items = vec![
      box_width(10.0),
      glue(),
      box_width(10.0),
      glue(),
      box_width(10.0),
      comma(5.0),
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(38.5), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines[0].boxes.len(), 4, "1 行目は句読点までの 4 箱: {lines:?}");
    assert_eq!(lines[0].hanging, Length::ZERO, "{lines:?}");
    assert!(lines[0].width().expect("値域内") <= pt(38.5), "{lines:?}");
  }

  #[test]
  fn last_line_hangs_without_stretching() {
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
    ];

    let lines = break_lines(&items, pt(32.0), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].boxes.last().expect("行は箱を持つ").dx, pt(30.0), "伸長しない: {lines:?}");
    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
  }

  #[test]
  fn line_before_a_forced_break_hangs_without_stretching() {
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      HItem::ForcedBreak,
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(32.0), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.last().expect("1 行目は箱を持つ").dx, pt(30.0), "{lines:?}");
    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
  }

  #[test]
  fn does_not_hang_a_comma_before_a_hyphenation_point() {
    // 分割点は語中点だけ。句読点を除けば本文 30 がハイフンぶん狭めた幅 30 にちょうど収まるので、ぶら下げを許せば実現可能になる
    let items = vec![
      box_width(10.0),
      box_width(10.0),
      box_width(10.0),
      comma(5.0),
      discretionary(1.0),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(31.0), pt(5.0)).expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
  }

  #[test]
  fn hangs_only_the_last_of_consecutive_commas() {
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      comma(5.0),
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(25.0), pt(5.0)).expect("テストの寸法は値域内");

    let dxs: Vec<Length> = lines[0].boxes.iter().map(|placed| return placed.dx).collect();
    assert_eq!(dxs, vec![pt(0.0), pt(10.0), pt(20.0), pt(25.0)], "{lines:?}");
    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
  }

  #[test]
  fn does_not_hang_when_the_body_still_overflows() {
    // 末尾の 1 字を除いても 25 > 24 で収縮点も無い → ぶら下げ候補にならず、句読点 2 字は前の字と一緒に次行へ
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      comma(5.0),
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(24.0), pt(5.0)).expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "{lines:?}");
  }

  #[test]
  fn does_not_hang_a_box_that_is_not_a_comma() {
    let items = vec![
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      box_width(5.0),
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(30.0), pt(5.0)).expect("テストの寸法は値域内");

    assert!(
      lines
        .iter()
        .all(|line| return line.hanging == Length::ZERO && line.width().expect("値域内") <= pt(30.0)),
      "{lines:?}"
    );
  }

  #[test]
  fn hangs_comma_before_link_end_and_extends_the_rect() {
    let items = vec![
      HItem::LinkStart(link_target()),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      cjk_glue(),
      box_width(10.0),
      comma(5.0),
      HItem::LinkEnd,
      comma_aki(),
      box_width(10.0),
    ];

    let lines = break_lines(&items, pt(30.0), pt(5.0)).expect("テストの寸法は値域内");

    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
    assert_eq!((lines[0].links[0].x0, lines[0].links[0].x1), (pt(0.0), pt(35.0)), "{lines:?}");
  }
}
