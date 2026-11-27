//! Knuth–Plass 方式の段落全体最適な行分割
//!
//! 経路のコストは行ごとの demerits（badness・ハイフン・数式内分割点）に、隣接する非最終行の疎密の差の罰点を足した和。
//! 疎密は配置後の字間・語間で測り（調整比 r の 3 区分 [`Fitness`]）、密な行と疎な行が上下に並ぶ組に
//! [`ADJACENT_FITNESS_DEMERIT`] を足す。伸長能力を超えて余る行は配置で伸長限界（r = 1）に止まるので疎な行と同じに扱い、
//! 右端の不足は badness の上限が受け持つ。伸縮点が無いのに余る行は字間を持たず、段落末・強制改行直前の行は自然幅で組むので、
//! どちらも区分を持たず隣と比べない（強制改行で区切ったサブ段落は独立に最適化するので、強制改行もまたがない）。
//! 罰点は区分だけで決まるので、DP の状態は「破断点 × 末尾の行の区分」で足りる。ぶら下げる案とぶら下げない案は行単体の
//! コストで選び、隣接行の区分を見ない。
//!
//! 段落の最後のサブ段落が 2 行以上になるとき、その最終行の字の幅（最初の字から最後の字までの自然幅）が
//! [`SHORT_LAST_LINE_EMS`] em に満たなければ、不足に比例した罰点を足す。罰点は最終行 1 本で決まるので状態を増やさない。
//! 1 行で完結するサブ段落と強制改行直前の行は測らない。

use tracing::trace;

use crate::{
  document::TextAlignment,
  length::{Length, LengthOverflow},
  style::OpenBracketAki,
  typeset::{
    boxes::{HItem, Line},
    boxing::break_lines::{
      AdjustmentOrder, LineEnd, OpenLink, build_line, glue_metrics, greedy, open_bracket_lead, strip_leading_glue,
      trailing_comma, trim_trailing_glue,
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
/// 隣接する 2 本の非最終行が疎な行と密な行の組のときに足す demerits（TeX の `\adjdemerits` 相当）
///
/// 語中ハイフン 1 つ（[`HYPHEN_DEMERIT`]）より重く、連続ハイフン（[`DOUBLE_HYPHEN_DEMERIT`]）と同じ重さ。区分の跳びより
/// ハイフンを選び、跳びを避けるためだけに連続ハイフンは作らない。badness では 1 行を b ≈ 90（r ≈ 0.97）まで粗くする
/// 差に当たる。
const ADJACENT_FITNESS_DEMERIT: f64 = 10_000.0;
/// 段落末尾の行の字の幅の下限（段落の文字サイズの em 単位）
///
/// 和文の 1〜2 字 + 句点（1.5〜2.5 em）は下回り、3 字以上は下回らない。欧文では小文字のおよそ 6 字に当たる。
const SHORT_LAST_LINE_EMS: f64 = 3.0;
/// 段落末尾の行に字が 1 つも無いときの demerits。字の幅が下限に足りない割合に比例して課す
///
/// [`ADJACENT_FITNESS_DEMERIT`] と同じ重さで、語中ハイフン 1 つ（[`HYPHEN_DEMERIT`]）より重い。
const SHORT_LAST_LINE_DEMERIT: f64 = 10_000.0;

/// Knuth–Plass 方式（段落全体最適）で水平リストを本文幅の両端揃えの行に分割する
///
/// 行末の句読点（[`HItem::Comma`]）は、その幅が `hang_room` 以下で、ぶら下げると行が実現可能になるか
/// より安くなるときだけ利用幅の外へぶら下げる。確定した行の伸縮は `order` の順位で配る。行頭の始め括弧の前アキは、
/// 最初のサブ段落の 1 行目が `paragraph_start`、ほかの行が `line_start` に従う。`last_line_em` は段落末尾の行の短さを
/// 測る em で、`None` なら測らない。
///
/// # Errors
///
/// 行の幅や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
pub(super) fn break_lines(
  items: &[HItem],
  text_width: Length,
  hang_room: Length,
  order: AdjustmentOrder,
  paragraph_start: OpenBracketAki,
  line_start: OpenBracketAki,
  last_line_em: Option<Length>,
) -> Result<Vec<Line>, LengthOverflow> {
  let last_line_floor = last_line_em.map(|em| return em.try_scale(SHORT_LAST_LINE_EMS)).transpose()?;
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
    let first_line = if index == 0 {
      paragraph_start
    } else {
      line_start
    };
    // 強制改行直前の行は段落末尾ではないので測らない
    let floor = if has_following_break {
      None
    } else {
      last_line_floor
    };
    let segment_lines =
      break_subparagraph(segment, text_width, hang_room, &mut open_links, order, first_line, line_start, floor)?;
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
  /// 経路上の demerits の総和（隣接行の疎密の差の罰点を含む）
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
    /// 疎密の区分。隣と比べない行（最終行・伸縮点の無い行）は `None`
    fitness: Option<Fitness>,
  },
  /// ぶら下げる案を含めて収縮能力を超えて溢れる（実現不能）。行頭を前へずらすと更に長くなるため早期打ち切りに使う
  Overflow,
}

/// 伸縮点を持つ非最終行の、配置後の疎密の区分（宣言順に疎 → 密）
///
/// 尺度は badness と同じ調整比 r。各 glue の伸縮能力は自分の文字サイズの em で決まるので、r は文字サイズにも
/// 和欧の比率にもよらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fitness {
  /// r > 0.5。r > 1 の行も配置では伸長限界（r = 1）で止まるのでここに入る
  Loose,
  /// −0.5 ≤ r ≤ 0.5
  Decent,
  /// r < −0.5（r ≥ −1 は可否判定が保証する）
  Tight,
}

impl Fitness {
  /// 伸縮点を持つ非最終行の調整比 `ratio` から区分を決める
  fn of_ratio(ratio: f64) -> Self {
    return if ratio > 0.5 {
      Fitness::Loose
    } else if ratio >= -0.5 {
      Fitness::Decent
    } else {
      Fitness::Tight
    };
  }
}

/// 上下に隣接する行の区分 `above` / `below` の組に課す追加 demerits
///
/// 疎な行と密な行（`Loose` と `Tight`。向きは問わない）の組なら [`ADJACENT_FITNESS_DEMERIT`]、それ以外は 0。どちらかが
/// `None`（最終行・伸縮点の無い行・段落頭の前）なら比べない。
fn adjacent_demerits(above: Option<Fitness>, below: Option<Fitness>) -> f64 {
  return match (above, below) {
    (Some(Fitness::Loose), Some(Fitness::Tight)) | (Some(Fitness::Tight), Some(Fitness::Loose)) => {
      ADJACENT_FITNESS_DEMERIT
    },
    (
      Some(Fitness::Loose | Fitness::Decent | Fitness::Tight),
      Some(Fitness::Loose | Fitness::Decent | Fitness::Tight),
    )
    | (None, _)
    | (_, None) => 0.0,
  };
}

/// DP の 1 状態へ到達する最良経路と、その末尾の行
#[derive(Debug, Clone, Copy)]
struct Reach {
  /// 経路全体のコスト
  cost: PathCost,
  /// 末尾の行の直前の状態（破断点の添字とその行の区分）。`None` で行頭（item 0）から始まる
  from: Option<(usize, Option<Fitness>)>,
  /// 末尾の行の疎密の区分（隣と比べない行は `None`）
  fitness: Option<Fitness>,
  /// 末尾の行単体の badness（TRACE 観測用。DP の選択には使わない）
  badness: f64,
  /// 末尾の行が行末の句読点をぶら下げる案か
  hanging: bool,
}

/// 強制改行を含まない 1 サブ段落を最適分割する。溢れる行を避けられない（末尾まで実現可能な分割が無い）サブ段落だけを
/// 貪欲法（`greedy::break_lines`）で組み、開いているリンクも貪欲法へ引き継ぐ。行頭の始め括弧の前アキは 1 行目が
/// `first_line`、2 行目以降が `line_start` に従う。`last_line_floor` は最終行の字の幅の下限（`None` なら測らない）
#[expect(
  clippy::too_many_arguments,
  reason = "サブ段落 1 つの組み方を決める互いに独立な値で、束ねても守る不変条件が無い"
)]
fn break_subparagraph(
  items: &[HItem],
  text_width: Length,
  hang_room: Length,
  open_links: &mut Vec<OpenLink>,
  order: AdjustmentOrder,
  first_line: OpenBracketAki,
  line_start: OpenBracketAki,
  last_line_floor: Option<Length>,
) -> Result<Vec<Line>, LengthOverflow> {
  let breaks = breakpoints(items);
  let Some(chain) = choose_path(items, &breaks, text_width, hang_room, first_line, line_start, last_line_floor)? else {
    // 末尾の仮想破断点へ到達できない（実現可能な分割が無い）ならフォールバック
    return greedy::break_lines(items, text_width, TextAlignment::Justify, open_links, order, first_line, line_start);
  };

  let mut lines: Vec<Line> = Vec::new();
  let mut start = 0usize;
  for (line_index, (node, reach)) in chain.iter().enumerate() {
    let brk = &breaks[*node];
    let refs: Vec<&HItem> = items[start..brk.at].iter().collect();
    let refs = strip_leading_glue(&refs);
    let end = if reach.hanging {
      LineEnd::Hang(trailing_comma(refs).expect("ぶら下げ案は行末が句読点の候補行にだけ作る"))
    } else {
      match items.get(brk.at) {
        Some(HItem::Discretionary { hyphen }) => LineEnd::Hyphen(hyphen),
        _ => LineEnd::Plain,
      }
    };
    let lead = open_bracket_lead(
      refs.iter().copied(),
      if line_index == 0 {
        first_line
      } else {
        line_start
      },
    );
    let mut line =
      build_line(refs, brk.is_end, text_width.try_sub(lead)?, TextAlignment::Justify, open_links, end, order)?;
    line.shift_x(lead)?;
    let width = line.width()?;
    // `line_index` はサブ段落内の連番（強制改行ごとに 0 へ戻る）。段落通し番号ではない
    trace!(
      line_index,
      break_at = brk.at,
      is_last = brk.is_end,
      is_hyphenated = brk.hyphen,
      is_math_break = brk.math_penalty.is_some(),
      is_hanging = reach.hanging,
      badness = reach.badness,
      fitness = ?reach.fitness,
      width_pt = %width.to_pt(),
      text = observe::summarize_line(&line),
      "行を確定"
    );
    lines.push(line);
    start = brk.at + 1;
  }
  return Ok(lines);
}

/// 合法破断点を item 順に列挙し、末尾にサブ段落末尾の仮想破断点を足す
fn breakpoints(items: &[HItem]) -> Vec<Breakpoint> {
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
      | HItem::OpenBracket { .. }
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
  return breaks;
}

/// 段落全体で経路コスト（[`PathCost`]）が最小の破断点の列を選ぶ
///
/// 末尾の仮想破断点へ到達できなければ `None`。到達すれば行順の（`breaks` の添字, その行で終わる [`Reach`]）の列で、
/// 最後の要素の `cost` が経路全体のコスト。行頭の始め括弧の前アキは 1 行目が `first_line`、2 行目以降が `line_start`
/// に従う。`last_line_floor` が `Some` なら、2 行目以降として末尾の仮想破断点で終わる行に [`last_line_demerits`] を足す
/// （行頭から末尾までの 1 行は測らない）。
///
/// # Errors
///
/// 行の幅や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
fn choose_path(
  items: &[HItem],
  breaks: &[Breakpoint],
  text_width: Length,
  hang_room: Length,
  first_line: OpenBracketAki,
  line_start: OpenBracketAki,
  last_line_floor: Option<Length>,
) -> Result<Option<Vec<(usize, Reach)>>, LengthOverflow> {
  // best[j][state_slot(f)] = 破断点 breaks[j] で区分 f の行を終える最良の到達（到達不能なら None）。隣接行の罰点は
  // 区分だけで決まるので、区分ごとに最良を 1 つ残せば段落全体の最良を取りこぼさない
  let node_count = breaks.len();
  let mut best: Vec<[Option<Reach>; STATE_SLOTS]> = vec![[None; STATE_SLOTS]; node_count];

  for j in 0..node_count {
    // 行頭候補を近い側（短い行）から見て、溢れたら以降（より長い行）は不要なので打ち切る
    let mut overflowed = false;
    for i in (0..j).rev() {
      let start = breaks[i].at + 1;
      match edge_cost(items, start, &breaks[j], breaks[i].hyphen, text_width, hang_room, line_start)? {
        Edge::Overflow => {
          overflowed = true;
          break;
        },
        Edge::Feasible {
          demerits,
          badness,
          rigid,
          hanging,
          fitness,
          ..
        } => {
          let last_line = match last_line_floor {
            Some(floor) if breaks[j].is_end => last_line_demerits(items, start, breaks[j].at, floor)?,
            _ => 0.0,
          };
          let predecessors = best[i];
          for from in predecessors.iter().flatten() {
            relax(
              &mut best[j][state_slot(fitness)],
              Reach {
                cost: from.cost.then(
                  &breaks[j],
                  demerits + adjacent_demerits(from.fitness, fitness) + last_line,
                  rigid,
                ),
                from: Some((i, from.fitness)),
                fitness,
                badness,
                hanging,
              },
            );
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
        fitness,
        ..
      } = edge_cost(items, 0, &breaks[j], false, text_width, hang_room, first_line)?
    {
      relax(
        &mut best[j][state_slot(fitness)],
        Reach {
          cost: PathCost::START.then(&breaks[j], demerits, rigid),
          from: None,
          fitness,
          badness,
          hanging,
        },
      );
    }
  }

  // 末尾の仮想破断点で終わる行は最終行なので、区分なしの状態に入る
  let Some(mut reach) = best[node_count - 1][state_slot(None)] else {
    return Ok(None);
  };
  let mut chain: Vec<(usize, Reach)> = Vec::new();
  let mut node = node_count - 1;
  loop {
    chain.push((node, reach));
    let Some((from, fitness)) = reach.from else {
      break;
    };
    node = from;
    reach = best[from][state_slot(fitness)].expect("採用した到達の直前の状態は到達済み");
  }
  chain.reverse();
  return Ok(Some(chain));
}

/// DP の状態（行の区分。`None` は隣と比べない行）の数
const STATE_SLOTS: usize = 4;

/// 行の区分 `fitness` の DP 状態の添字
fn state_slot(fitness: Option<Fitness>) -> usize {
  return match fitness {
    None => 0,
    Some(Fitness::Loose) => 1,
    Some(Fitness::Decent) => 2,
    Some(Fitness::Tight) => 3,
  };
}

/// `slot` の到達より `candidate` のコストが小さければ置き換える（同値は先に入った方を残す）
fn relax(slot: &mut Option<Reach>, candidate: Reach) {
  if slot.is_none_or(|current| return candidate.cost < current.cost) {
    *slot = Some(candidate);
  }
}

/// 候補行のコストを評価し、結果を TRACE へ出す
fn edge_cost(
  items: &[HItem],
  line_start: usize,
  brk: &Breakpoint,
  prev_hyphen: bool,
  text_width: Length,
  hang_room: Length,
  aki: OpenBracketAki,
) -> Result<Edge, LengthOverflow> {
  let edge = evaluate_edge(items, line_start, brk, prev_hyphen, text_width, hang_room, aki)?;
  let (outcome, demerits, badness, ratio, is_hanging, fitness) = match &edge {
    Edge::Feasible {
      demerits,
      badness,
      ratio,
      rigid,
      hanging,
      fitness,
    } => (
      if *rigid { "rigid" } else { "feasible" },
      Some(*demerits),
      Some(*badness),
      Some(*ratio),
      *hanging,
      *fitness,
    ),
    Edge::Overflow => ("overflow", None, None, None, false, None),
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
    fitness = ?fitness,
    "行分割の候補を評価"
  );
  return Ok(edge);
}

/// 候補行（`items[line_start..brk.at]`）の demerits を評価する
///
/// 自然幅が利用幅を溢れ、行末が幅 `hang_room` 以下の句読点で語中破断でない行は、句読点をぶら下げる案も評価する。
/// 両案とも実現可能なら伸縮点の無い行か → demerits の順に小さい方（同値はぶら下げない案）を採る。
/// 行頭の始め括弧の前アキ（`aki` に従う）は利用幅から引く。
fn evaluate_edge(
  items: &[HItem],
  line_start: usize,
  brk: &Breakpoint,
  prev_hyphen: bool,
  text_width: Length,
  hang_room: Length,
  aki: OpenBracketAki,
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
  let available = text_width.try_sub(hyphen_width)?.try_sub(open_bracket_lead(refs.iter().copied(), aki))?;
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
///
/// 能力は行全体の合計で見る。配置（`build_line`）は同じ合計を順位ごとに使い切るだけなので、ここで実現可能とした行は
/// 配置でも利用幅に揃う。
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
      fitness: None,
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
          fitness: None,
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
    fitness: Some(Fitness::of_ratio(ratio)),
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

/// 段落末尾の行（`items[line_start..end]`）に課す短さの罰点を求め、TRACE へ出す
///
/// 行の範囲は候補行の評価（[`evaluate_edge`]）と同じく、行頭と行末の breakable glue を除く。
///
/// # Errors
///
/// 字の幅の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
fn last_line_demerits(items: &[HItem], line_start: usize, end: usize, floor: Length) -> Result<f64, LengthOverflow> {
  let refs: Vec<&HItem> = items[line_start..end].iter().collect();
  let extent = last_line_extent(trim_trailing_glue(strip_leading_glue(&refs)))?;
  let demerits = short_last_line_demerits(extent, floor);
  trace!(
    line_start,
    extent_pt = %extent.to_pt(),
    floor_pt = %floor.to_pt(),
    demerits,
    "段落末尾の行の短さを評価"
  );
  return Ok(demerits);
}

/// 行の字の幅（最初の字から最後の字までの自然幅）を返す。字が無ければ 0
///
/// 字は本文の箱（[`HItem::Box`]・[`HItem::Comma`]・[`HItem::OpenBracket`] の箱。始め括弧の前アキは含まない）。両端の
/// 固定カーン（明示アキ）・[`HItem::FlushRight`]（証明終端記号）・幅 0 の運搬マーカーは数えず、字の間の glue・カーン・
/// 折らなかった数式内分割点のアキは数える。
///
/// # Errors
///
/// 幅の和が長さの値域を超えたら [`LengthOverflow`] を返す。
fn last_line_extent(refs: &[&HItem]) -> Result<Length, LengthOverflow> {
  let is_ink = |item: &&HItem| return matches!(item, HItem::Box(_) | HItem::Comma(_) | HItem::OpenBracket { .. });
  let (Some(first), Some(last)) = (refs.iter().position(is_ink), refs.iter().rposition(is_ink)) else {
    return Ok(Length::ZERO);
  };
  return Ok(glue_metrics(&refs[first..=last])?.0);
}

/// 字の幅 `extent` の段落末尾の行に課す demerits
///
/// 下限 `floor` 以上なら 0、それ未満なら [`SHORT_LAST_LINE_DEMERIT`] × 不足の割合（`floor` に対する）。
fn short_last_line_demerits(extent: Length, floor: Length) -> f64 {
  if extent >= floor {
    return 0.0;
  }
  return SHORT_LAST_LINE_DEMERIT * (1.0 - extent.ratio(floor));
}

#[cfg(test)]
mod tests {
  use proptest::prelude::*;

  use super::{
    ADJACENT_FITNESS_DEMERIT, AdjustmentOrder, Breakpoint, Edge, Fitness, PathCost, SHORT_LAST_LINE_DEMERIT,
    adjacent_demerits, break_lines, break_subparagraph, breakpoints, choose_path, demerits, evaluate_edge,
    last_line_demerits, last_line_extent, short_last_line_demerits,
  };
  use crate::{
    document::TextAlignment,
    length::Length,
    style::OpenBracketAki,
    typeset::{
      boxes::{GlueKind, HItem, Line},
      boxing::break_lines::{
        greedy,
        test_support::{
          box_width, cjk_glue, comma, comma_aki, discretionary, flush_right_box, hang_items, index_mark, link_target,
          math_break, open_bracket, stretch_glue, test_box,
        },
      },
    },
  };

  /// 行頭の始め括弧の前アキなし（ここのテストは始め括弧を積まない）
  const NONE: OpenBracketAki = OpenBracketAki::None;

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
    let lines = break_lines(&[], Length::pt(100.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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
        kind: GlueKind::WordSpace,
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

    let greedy = greedy::break_lines(
      &items,
      Length::pt(25.0),
      TextAlignment::Justify,
      &mut Vec::new(),
      AdjustmentOrder::Uniform,
      NONE,
      NONE,
    )
    .expect("テストの寸法は値域内");
    let kp = break_lines(&items, Length::pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(27.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

    // glue は自然幅 5 のまま
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(close(lines[0].boxes[1].dx, 15.0), "{lines:?}");
  }

  #[test]
  fn empty_segment_before_forced_break_yields_empty_line() {
    let items = vec![HItem::ForcedBreak, test_box()];

    let lines = break_lines(&items, Length::pt(100.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].boxes.is_empty(), "1 行目は空: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1);
  }

  #[test]
  fn trailing_forced_break_does_not_add_empty_line() {
    let items = vec![test_box(), HItem::ForcedBreak];

    let lines = break_lines(&items, Length::pt(100.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(14.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].boxes.len(), 1, "1 行目は本文 box のみ: {lines:?}");
    assert_eq!(lines[1].boxes.len(), 1, "2 行目は QED のみ: {lines:?}");
    assert!(close(lines[1].boxes[0].dx, 6.0), "QED は右端寄せ: {lines:?}");
  }

  #[test]
  fn flush_right_box_sits_on_last_line_when_it_fits() {
    let items = vec![test_box(), stretch_glue(), flush_right_box(8.0)];

    let lines = break_lines(&items, Length::pt(50.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, pt(30.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, pt(30.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, pt(30.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, pt(30.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(27.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(40.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(34.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let kp = break_lines(&items, Length::pt(30.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");
    let greedy = greedy::break_lines(
      &items,
      Length::pt(30.0),
      TextAlignment::Justify,
      &mut Vec::new(),
      AdjustmentOrder::Uniform,
      NONE,
      NONE,
    )
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
    let lines = break_subparagraph(
      &items,
      Length::pt(27.0),
      Length::ZERO,
      &mut open_links,
      AdjustmentOrder::Uniform,
      NONE,
      NONE,
      None,
    )
    .expect("テストの寸法は値域内");

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
        kind: GlueKind::WordSpace,
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

    let kp = break_lines(&items, pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");
    let greedy = greedy::break_lines(
      &items,
      pt(25.0),
      TextAlignment::Justify,
      &mut Vec::new(),
      AdjustmentOrder::Uniform,
      NONE,
      NONE,
    )
    .expect("テストの寸法は値域内");

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
        kind: GlueKind::WordSpace,
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

    let lines = break_lines(&items, pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let exact = evaluate_edge(&items, 0, &brk(1), false, pt(25.0), Length::ZERO, NONE).expect("テストの寸法は値域内");
    let short = evaluate_edge(&items, 2, &brk(3), false, pt(25.0), Length::ZERO, NONE).expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(36.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(40.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, Length::pt(32.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(30.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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
    let lines = break_lines(&hang_items(), pt(30.0), pt(4.99), AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
    assert!(lines[0].width().expect("値域内") <= pt(30.0), "{lines:?}");
  }

  #[test]
  fn does_not_hang_a_comma_that_fits_at_natural_width() {
    let lines = break_lines(&hang_items(), pt(35.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.hanging == Length::ZERO), "{lines:?}");
  }

  #[test]
  fn hanging_line_stretches_its_body_to_the_full_width() {
    // 句読点を除いた本文 30 を幅 30.5 へ伸ばす（和文字間 glue 2 本の伸長 1 のうち 0.5 を使う）
    let lines = break_lines(&hang_items(), pt(30.5), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None)
      .expect("テストの寸法は値域内");

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
        kind: GlueKind::WordSpace,
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

    let lines =
      break_lines(&items, pt(38.5), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(32.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(32.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(31.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(25.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(24.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(30.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, pt(30.0), pt(5.0), AdjustmentOrder::Uniform, NONE, NONE, None).expect("テストの寸法は値域内");

    assert_eq!(lines[0].hanging, pt(5.0), "{lines:?}");
    assert_eq!((lines[0].links[0].x0, lines[0].links[0].x1), (pt(0.0), pt(35.0)), "{lines:?}");
  }

  #[test]
  fn fitness_follows_the_ratio_boundaries() {
    assert_eq!(Fitness::of_ratio(-1.0), Fitness::Tight);
    assert_eq!(Fitness::of_ratio(-0.51), Fitness::Tight);
    assert_eq!(Fitness::of_ratio(-0.5), Fitness::Decent);
    assert_eq!(Fitness::of_ratio(0.0), Fitness::Decent);
    assert_eq!(Fitness::of_ratio(0.5), Fitness::Decent);
    assert_eq!(Fitness::of_ratio(0.51), Fitness::Loose);
    assert_eq!(Fitness::of_ratio(1.0), Fitness::Loose);
    // 伸長能力を超える行は配置で伸長限界（r = 1）に止まるので、字間は r = 1 の行と同じ
    assert_eq!(Fitness::of_ratio(1.01), Fitness::Loose);
    assert_eq!(Fitness::of_ratio(3.0), Fitness::Loose);
  }

  #[test]
  fn rigid_and_last_lines_are_not_compared() {
    let items = [box_width(10.0)];
    let brk = |is_end| {
      return Breakpoint {
        at: 1,
        hyphen: false,
        math_penalty: None,
        is_end,
      };
    };

    let rigid = evaluate_edge(&items, 0, &brk(false), false, pt(30.0), Length::ZERO, NONE).expect("値域内");
    let last = evaluate_edge(&items, 0, &brk(true), false, pt(30.0), Length::ZERO, NONE).expect("値域内");

    assert!(
      matches!(
        rigid,
        Edge::Feasible {
          rigid: true,
          fitness: None,
          ..
        }
      ),
      "伸縮点の無い余る行は字間を持たないので比べない"
    );
    assert!(matches!(last, Edge::Feasible { fitness: None, .. }), "最終行は区分なし");
  }

  /// 総当たりで、末尾まで実現可能な分割の列のうち最小の経路コストを求める（行頭の始め括弧の前アキなし）。`floor` が
  /// `Some` なら、2 行目以降の最終行に短さの罰点を足す
  fn brute_force_cost(
    items: &[HItem],
    breaks: &[Breakpoint],
    text_width: Length,
    hang_room: Length,
    floor: Option<Length>,
  ) -> Option<PathCost> {
    let inner = breaks.len() - 1;
    let mut best: Option<PathCost> = None;
    for mask in 0u32..(1u32 << inner) {
      let mut chosen: Vec<&Breakpoint> =
        (0..inner).filter(|i| return mask & (1 << i) != 0).map(|i| return &breaks[i]).collect();
      chosen.push(&breaks[inner]);
      let mut cost = PathCost::START;
      let mut start = 0usize;
      let mut prev_hyphen = false;
      let mut prev_fitness: Option<Fitness> = None;
      let mut feasible = true;
      for brk in chosen {
        match evaluate_edge(items, start, brk, prev_hyphen, text_width, hang_room, NONE).expect("値域内") {
          Edge::Overflow => {
            feasible = false;
            break;
          },
          Edge::Feasible {
            demerits,
            rigid,
            fitness,
            ..
          } => {
            let last_line = match floor {
              Some(floor) if brk.is_end && start != 0 => {
                last_line_demerits(items, start, brk.at, floor).expect("値域内")
              },
              _ => 0.0,
            };
            cost = cost.then(brk, demerits + adjacent_demerits(prev_fitness, fitness) + last_line, rigid);
            prev_fitness = fitness;
          },
        }
        start = brk.at + 1;
        prev_hyphen = brk.hyphen;
      }
      if feasible && best.is_none_or(|current| return cost < current) {
        best = Some(cost);
      }
    }
    return best;
  }

  /// 総当たりで比べられる小さい段落（box・glue・語中分割点・数式内分割点・句読点）
  fn small_paragraph() -> impl Strategy<Value = Vec<HItem>> {
    let kinds = [
      GlueKind::WordSpace,
      GlueKind::JaLatinAki,
      GlueKind::InterCharacter,
      GlueKind::MiddleDotAki,
      GlueKind::PunctuationAki,
    ];
    // 収縮は自然幅を超えない（実際の glue と同じ。行を長くすると溢れが減らない前提を DP の打ち切りが使う）
    let glue = (
      0usize..5,
      0.0f32..8.0,
      prop_oneof![Just(0.0f32), 0.0f32..4.0],
      prop_oneof![Just(0.0f32), 0.0f32..=1.0],
      any::<bool>(),
    )
      .prop_map(move |(kind, natural, stretch, shrink_fraction, breakable)| {
        return HItem::Glue {
          natural: pt(natural),
          stretch: pt(stretch),
          shrink: pt(natural * shrink_fraction),
          breakable,
          kind: kinds[kind],
        };
      });
    let item = prop_oneof![
      (1.0f32..30.0).prop_map(box_width),
      glue,
      (1.0f32..4.0).prop_map(discretionary),
      (0.0f32..3.0, 0i32..1000).prop_map(|(spacing, penalty)| return math_break(spacing, penalty)),
      (2.0f32..6.0).prop_map(comma),
    ];
    return prop::collection::vec(item, 1..12);
  }

  proptest! {
    // 罰点が効くのは 3 行以上の段落だけで、小さい乱択入力では稀なので、既定（256）より多く回す
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn chosen_path_matches_brute_force_minimum(
      items in small_paragraph(),
      width in 10.0f32..80.0,
      hang in prop::sample::select(vec![0.0f32, 5.0]),
      floor in prop::sample::select(vec![None, Some(12.0f32)]),
    ) {
      let breaks = breakpoints(&items);
      let floor = floor.map(pt);

      let chosen = choose_path(&items, &breaks, pt(width), pt(hang), NONE, NONE, floor).expect("値域内");
      let brute = brute_force_cost(&items, &breaks, pt(width), pt(hang), floor);

      prop_assert_eq!(chosen.map(|path| return path.last().expect("到達した経路は 1 行以上").1.cost), brute);
    }

    #[test]
    fn last_line_penalty_keeps_rigid_lines_and_math_breaks(
      items in small_paragraph(),
      width in 10.0f32..80.0,
    ) {
      let breaks = breakpoints(&items);
      // 罰点は demerits にだけ入るので、伸縮点の無い行の本数と数式内分割の回数の最小は罰点の有無で変わらない
      let priorities = |floor: Option<Length>| {
        return choose_path(&items, &breaks, pt(width), Length::ZERO, NONE, NONE, floor).expect("値域内").map(|path| {
          let cost = path.last().expect("到達した経路は 1 行以上").1.cost;
          return (cost.rigid_lines, cost.math_breaks);
        });
      };

      prop_assert_eq!(priorities(Some(pt(30.0))), priorities(None));
    }
  }

  /// 箱（pt 幅）を単語間スペース相当の glue で区切った欧文の段落
  fn latin(widths: &[f32]) -> Vec<HItem> {
    let mut items = Vec::new();
    for (index, width) in widths.iter().enumerate() {
      if index > 0 {
        items.push(stretch_glue());
      }
      items.push(box_width(*width));
    }
    return items;
  }

  /// 和文の段落。`o` = 字（幅 10）、`,` = 句読点（幅 5、後ろに約物アキ）、`|` = 次の字間を分割不可にする印
  fn cjk(pattern: &str) -> Vec<HItem> {
    let mut items = Vec::new();
    // 直前の箱が句読点か（最初の箱の前は None）
    let mut after_comma: Option<bool> = None;
    let mut no_break = false;
    for mark in pattern.chars() {
      if mark == '|' {
        no_break = true;
        continue;
      }
      match after_comma {
        Some(true) => items.push(comma_aki()),
        Some(false) if no_break => items.push(HItem::Glue {
          natural: Length::ZERO,
          stretch: pt(0.5),
          shrink: Length::ZERO,
          breakable: false,
          kind: GlueKind::InterCharacter,
        }),
        Some(false) => items.push(cjk_glue()),
        None => {},
      }
      let is_comma = mark == ',';
      items.push(if is_comma {
        comma(5.0)
      } else {
        box_width(10.0)
      });
      after_comma = Some(is_comma);
      no_break = false;
    }
    return items;
  }

  /// 和欧混植の段落。`None` = 字（幅 10）、`Some(w)` = 欧文語（幅 w）。字–字は和文字間、語–語は単語間スペース、
  /// 字–語は分割不可の和欧文間アキ（自然幅 2.5・伸長 0.5）で区切る
  fn mixed(tokens: &[Option<f32>]) -> Vec<HItem> {
    let mut items = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
      if index > 0 {
        items.push(match (tokens[index - 1], token) {
          (None, None) => cjk_glue(),
          (Some(_), Some(_)) => stretch_glue(),
          _ => HItem::Glue {
            natural: pt(2.5),
            stretch: pt(0.5),
            shrink: Length::ZERO,
            breakable: false,
            kind: GlueKind::JaLatinAki,
          },
        });
      }
      items.push(box_width(token.unwrap_or(10.0)));
    }
    return items;
  }

  /// 語（音節の箱の pt 幅の列）を単語間スペースで、語の中の音節を幅 3 のハイフンの語中分割点で区切った段落
  fn hyphenated(words: &[&[f32]]) -> Vec<HItem> {
    let mut items = Vec::new();
    for (index, syllables) in words.iter().enumerate() {
      if index > 0 {
        items.push(stretch_glue());
      }
      for (position, width) in syllables.iter().enumerate() {
        if position > 0 {
          items.push(discretionary(3.0));
        }
        items.push(box_width(*width));
      }
    }
    return items;
  }

  /// 各行の箱の数
  fn box_counts(lines: &[Line]) -> Vec<usize> { return lines.iter().map(|line| return line.boxes.len()).collect(); }

  /// 隣接する 2 行の区分の組に罰点が付くか（f64 の等値比較を避ける）
  fn penalized(above: Option<Fitness>, below: Option<Fitness>) -> bool { return adjacent_demerits(above, below) > 0.0; }

  /// 欧文の代表例（幅 70）。罰点なしなら [3][5][1] 語（r = 0.8 → −0.6）
  const LATIN_WIDTHS: [f32; 9] = [9.0, 24.0, 23.0, 9.0, 13.0, 12.0, 16.0, 4.0, 5.0];

  #[test]
  fn latin_avoids_a_loose_line_followed_by_a_tight_line() {
    let items = latin(&LATIN_WIDTHS);

    let lines =
      break_lines(&items, pt(70.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    assert_eq!(box_counts(&lines), [3, 4, 2], "{lines:?}");
  }

  #[test]
  fn japanese_avoids_a_loose_line_followed_by_a_tight_line() {
    let items = cjk(",oo|ooooooooooo,o|oo|o|oo,oooooo,,ooooo|o");

    let lines =
      break_lines(&items, pt(150.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    // 罰点なしなら [15][16][5]（r ≈ 0.769 → −0.667）
    assert_eq!(box_counts(&lines), [15, 15, 6], "{lines:?}");
  }

  #[test]
  fn mixed_script_avoids_a_tight_line_after_a_loose_line() {
    const J: Option<f32> = None;
    let items = mixed(&[
      J,
      J,
      Some(28.0),
      Some(12.0),
      Some(28.0),
      Some(16.0),
      J,
      J,
      Some(16.0),
      J,
      Some(12.0),
      Some(12.0),
      Some(16.0),
      Some(12.0),
      J,
      J,
      J,
      J,
      Some(24.0),
      J,
    ]);

    let lines =
      break_lines(&items, pt(140.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    // 罰点なしなら [7][10][3]（r ≈ 0.667 → −0.6）
    assert_eq!(box_counts(&lines), [7, 9, 4], "{lines:?}");
  }

  #[test]
  fn prefers_one_hyphen_over_a_fitness_jump() {
    let items = hyphenated(&[
      &[7.0],
      &[11.0],
      &[9.0, 10.0],
      &[4.0],
      &[11.0, 11.0],
      &[13.0],
      &[8.0, 9.0],
      &[8.0, 9.0],
    ]);

    let lines =
      break_lines(&items, pt(60.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    // 罰点なしなら 2 行目は 11 11 13 8 9（r = −0.6）でハイフンなし
    assert_eq!(box_widths(&lines[1]), [pt(11.0), pt(11.0), pt(13.0), pt(8.0), pt(3.0)], "{lines:?}");
    assert_eq!(box_widths(&lines[2]), [pt(9.0), pt(8.0), pt(9.0)], "{lines:?}");
  }

  #[test]
  fn adjacent_demerits_penalize_only_two_or_more_classes_apart() {
    assert!(
      (adjacent_demerits(Some(Fitness::Loose), Some(Fitness::Tight)) - ADJACENT_FITNESS_DEMERIT).abs() < f64::EPSILON
    );
    assert!(penalized(Some(Fitness::Tight), Some(Fitness::Loose)), "向きによらない");
    assert!(!penalized(Some(Fitness::Decent), Some(Fitness::Tight)));
    assert!(!penalized(Some(Fitness::Loose), Some(Fitness::Decent)));
    assert!(!penalized(Some(Fitness::Loose), Some(Fitness::Loose)));
  }

  #[test]
  fn adjacent_demerits_skips_unjudged_lines() {
    assert!(!penalized(Some(Fitness::Loose), None), "最終行・伸縮点の無い行は比べない");
    assert!(!penalized(None, Some(Fitness::Tight)), "段落頭は比べない");
  }

  #[test]
  fn forced_break_separates_fitness_comparison() {
    let single = latin(&LATIN_WIDTHS);
    let mut items = single.clone();
    items.push(HItem::ForcedBreak);
    items.extend(single.iter().cloned());

    let joined =
      break_lines(&items, pt(70.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");
    let alone =
      break_lines(&single, pt(70.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    assert_eq!(box_counts(&joined), [box_counts(&alone), box_counts(&alone)].concat(), "{joined:?}");
  }

  #[test]
  fn markers_do_not_change_the_fitness_choice() {
    let mut items = latin(&LATIN_WIDTHS);
    // 箱は偶数番目。3 語目（index 4）をリンクで包み、5 語目（index 8）の後ろに索引語を置く
    items.insert(9, index_mark("x", None));
    items.insert(5, HItem::LinkEnd);
    items.insert(4, HItem::LinkStart(link_target()));

    let lines =
      break_lines(&items, pt(70.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, None).expect("値域内");

    assert_eq!(box_counts(&lines), [3, 4, 2], "{lines:?}");
  }

  /// 最終行の短さを測る em（下限は 3 em = 30pt）
  const EM: Option<Length> = Some(Length::from_sp(10 * 65_536));

  /// 行の字の幅の自然幅（`last_line_extent` を item 列へ当てる短縮子）
  fn extent(items: &[HItem]) -> Length {
    let refs: Vec<&HItem> = items.iter().collect();
    return last_line_extent(&refs).expect("値域内");
  }

  #[test]
  fn last_line_extent_spans_first_to_last_ink() {
    let items = [
      HItem::Kern(pt(5.0)),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      HItem::Kern(pt(7.0)),
      flush_right_box(8.0),
      index_mark("x", None),
    ];

    // 両端のカーン・証明終端記号・索引マーカーを除き、字の間の glue の自然幅 5 を数える
    assert_eq!(extent(&items), pt(25.0));
  }

  #[test]
  fn last_line_extent_counts_comma_and_open_bracket_boxes() {
    let items = [open_bracket(10.0, 5.0), box_width(10.0), comma(5.0)];

    // 始め括弧の前アキ（lead 5）は行のずらしなので数えない
    assert_eq!(extent(&items), pt(25.0));
  }

  #[test]
  fn last_line_extent_without_ink_is_zero() {
    assert_eq!(extent(&[flush_right_box(8.0)]), Length::ZERO);
    assert_eq!(extent(&[]), Length::ZERO);
  }

  /// 罰点が `expected` に（差 1e-6 未満で）一致するか（f64 の等値比較を避ける）
  fn demerits_close(actual: f64, expected: f64) -> bool { return (actual - expected).abs() < 1e-6; }

  #[test]
  fn short_last_line_demerits_are_proportional_to_the_shortfall() {
    assert!(demerits_close(short_last_line_demerits(pt(30.0), pt(30.0)), 0.0), "下限ちょうどは罰しない");
    assert!(demerits_close(short_last_line_demerits(pt(40.0), pt(30.0)), 0.0));
    assert!(demerits_close(short_last_line_demerits(pt(15.0), pt(30.0)), SHORT_LAST_LINE_DEMERIT / 2.0));
    assert!(demerits_close(short_last_line_demerits(Length::ZERO, pt(30.0)), SHORT_LAST_LINE_DEMERIT));
  }

  #[test]
  fn last_line_demerits_measure_the_line_without_its_edge_glue() {
    // 行頭の breakable glue（index 1）と行末の breakable glue を除いた [b10] の行
    let items = [
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
    ];

    let demerits = last_line_demerits(&items, 1, items.len(), pt(30.0)).expect("値域内");

    assert!(demerits_close(demerits, SHORT_LAST_LINE_DEMERIT * 2.0 / 3.0), "{demerits}");
  }

  /// 欧文の短い最終行の代表例（幅 70）。罰点なしなら [17 24 19][24 13 8 5 4][14]
  const SHORT_TAIL_WIDTHS: [f32; 9] = [17.0, 24.0, 19.0, 24.0, 13.0, 8.0, 5.0, 4.0, 14.0];

  /// 両端揃えの段落を幅 `width` で、最終行の短さを `em` で測って組む（ぶら下げ・始め括弧の前アキなし）
  fn justify(items: &[HItem], width: f32, em: Option<Length>) -> Vec<Line> {
    return break_lines(items, pt(width), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE, em).expect("値域内");
  }

  #[test]
  fn latin_avoids_a_last_line_of_one_short_word() {
    let lines = justify(&latin(&SHORT_TAIL_WIDTHS), 70.0, EM);

    // 罰点なしなら [3][5][1]: 2 行目 r = −0.6（demerits 998.56）、最終行の字の幅 14 に罰点 5 333。2 行目から 1 語を
    // 送ると r ≈ 0.667（1 570.5）、最終行 4 + 5 + 14 = 23 の罰点 2 333 で、総 demerits は 6 532 → 4 104
    assert_eq!(box_counts(&lines), [3, 4, 2], "{lines:?}");
  }

  #[test]
  fn japanese_pushes_characters_onto_a_one_character_last_line() {
    let lines = justify(&cjk(&"o".repeat(61)), 300.0, EM);

    // 罰点なしなら [30][30][1]（最終行の罰点 6 667）。29 字の行は字間伸長 14 に 10 余り r ≈ 0.714（demerits 2 153）。
    // 2 行から 1 字ずつ送れば最終行は 3 字 = 下限ちょうど。1 行から 2 字は送れない（r > 1）
    assert_eq!(box_counts(&lines), [29, 29, 3], "{lines:?}");
  }

  #[test]
  fn japanese_absorbs_a_one_character_last_line_by_shrinking_punctuation_aki() {
    let lines = justify(&cjk("oo|oooooo|ooo,oo,ooooo,oooooooo,o"), 150.0, EM);

    // 罰点なしなら [15][15][1]。1 行目の約物アキ 2 つを詰めて 1 字を追い込み、最終行が無くなる
    assert_eq!(box_counts(&lines), [16, 15], "{lines:?}");
  }

  #[test]
  fn hyphen_fragment_is_not_left_alone_on_the_last_line() {
    let items = hyphenated(&[
      &[5.0, 4.0, 6.0],
      &[8.0, 10.0, 9.0],
      &[10.0, 8.0],
      &[9.0],
      &[10.0],
      &[4.0, 11.0],
    ]);

    let lines = justify(&items, 60.0, EM);

    // 罰点なしなら 2 行目が語中で折れ（… 10 4 + ハイフン）、最終行は断片 11 だけ
    assert_eq!(box_widths(&lines[0]), [pt(5.0), pt(4.0), pt(6.0), pt(8.0), pt(3.0)], "{lines:?}");
    assert_eq!(box_widths(&lines[1]), [pt(10.0), pt(9.0), pt(10.0), pt(8.0), pt(9.0)], "{lines:?}");
    assert_eq!(box_widths(&lines[2]), [pt(10.0), pt(4.0), pt(11.0)], "{lines:?}");
  }

  #[test]
  fn mixed_script_avoids_a_short_last_line() {
    const J: Option<f32> = None;
    let items = mixed(&[
      J,
      Some(28.0),
      Some(9.0),
      J,
      Some(19.0),
      J,
      Some(29.0),
      J,
      J,
      J,
      J,
      Some(13.0),
      Some(19.0),
      Some(18.0),
      Some(14.0),
      J,
      Some(9.0),
      Some(10.0),
      Some(10.0),
    ]);

    let lines = justify(&items, 140.0, EM);

    // 罰点なしなら [2][7][9][1]（最終行は欧文語 10 だけ）
    assert_eq!(box_counts(&lines), [2, 6, 9, 2], "{lines:?}");
  }

  #[test]
  fn keeps_a_short_last_line_when_avoiding_it_needs_a_very_loose_line() {
    // 1・2 行目は自然幅ちょうど。2 行目から 1 語送ると 45 が 70 へ伸長 2.5 で r = 10（badness 上限）、最終行の 4 を
    // 2 行目へ入れると 79 が収縮 5/3 × 3 を超えて溢れる
    let lines = justify(&latin(&[20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 4.0]), 70.0, EM);

    assert_eq!(box_counts(&lines), [3, 3, 1], "{lines:?}");
  }

  #[test]
  fn keeps_a_short_last_line_without_an_alternative() {
    // 1 行では 53 が収縮 5/3 を超えて溢れ、分割点は空白 1 つだけ
    let lines = justify(&latin(&[40.0, 8.0]), 45.0, EM);

    assert_eq!(box_counts(&lines), [1, 1], "{lines:?}");
  }

  #[test]
  fn single_line_subparagraph_is_not_judged() {
    let items = [box_width(8.0)];
    let breaks = breakpoints(&items);

    let chain = choose_path(&items, &breaks, pt(70.0), Length::ZERO, NONE, NONE, Some(pt(30.0)))
      .expect("値域内")
      .expect("1 行に収まる");

    assert_eq!(chain.len(), 1);
    assert!(
      demerits_close(chain[0].1.cost.demerits, demerits(0.0, false, false, None)),
      "字の幅 8 の 1 行でも罰点を足さない: {chain:?}"
    );
  }

  /// [`SHORT_TAIL_WIDTHS`] の段落を強制改行でつないだ 2 サブ段落
  fn two_short_tail_subparagraphs() -> Vec<HItem> {
    let mut items = latin(&SHORT_TAIL_WIDTHS);
    items.push(HItem::ForcedBreak);
    items.extend(latin(&SHORT_TAIL_WIDTHS));
    return items;
  }

  #[test]
  fn line_before_a_forced_break_is_not_judged() {
    let lines = justify(&two_short_tail_subparagraphs(), 70.0, EM);

    assert_eq!(box_counts(&lines)[..3], box_counts(&justify(&latin(&SHORT_TAIL_WIDTHS), 70.0, None)), "{lines:?}");
  }

  #[test]
  fn last_subparagraph_after_a_forced_break_is_judged() {
    let lines = justify(&two_short_tail_subparagraphs(), 70.0, EM);

    assert_eq!(box_counts(&lines)[3..], [3, 4, 2], "{lines:?}");
  }

  #[test]
  fn qed_does_not_lengthen_the_last_line() {
    // 証明終端記号（幅 20）を字の幅に数えると、最終行 [14 ∎] は 34 で下限を越えて [3][5][2] のまま残る
    let mut items = latin(&SHORT_TAIL_WIDTHS);
    items.push(HItem::Penalty { value: 0 });
    items.push(flush_right_box(20.0));

    let lines = justify(&items, 70.0, EM);

    assert_eq!(box_widths(&lines[2]), [pt(4.0), pt(14.0), pt(20.0)], "{lines:?}");
  }

  #[test]
  fn trailing_kern_does_not_lengthen_the_last_line() {
    let mut items = latin(&SHORT_TAIL_WIDTHS);
    items.push(HItem::Kern(pt(30.0)));

    let lines = justify(&items, 70.0, EM);

    assert_eq!(box_counts(&lines), [3, 4, 2], "末尾の明示アキを字の幅に数えない: {lines:?}");
  }

  #[test]
  fn markers_do_not_change_the_last_line_choice() {
    let mut items = latin(&SHORT_TAIL_WIDTHS);
    // 箱は偶数番目。8 語目（index 14）をリンクで包み、最後の語の後ろに索引語を置く
    items.push(index_mark("x", None));
    items.insert(15, HItem::LinkEnd);
    items.insert(14, HItem::LinkStart(link_target()));

    let lines = justify(&items, 70.0, EM);

    assert_eq!(box_counts(&lines), [3, 4, 2], "{lines:?}");
  }

  #[test]
  fn narrow_width_still_breaks() {
    // 利用幅 20 は下限 30 より狭く、どの最終行も下限に届かない
    let items = latin(&[8.0, 8.0, 8.0, 8.0]);

    let judged = justify(&items, 20.0, EM);

    assert_eq!(box_counts(&judged), box_counts(&justify(&items, 20.0, None)), "{judged:?}");
  }
}
