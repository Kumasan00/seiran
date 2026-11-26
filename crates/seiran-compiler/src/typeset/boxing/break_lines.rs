//! 行分割

mod greedy;
mod knuth_plass;

use std::cmp::Ordering;

use crate::{
  document::TextAlignment,
  length::{Length, LengthOverflow},
  style::OpenBracketAki,
  typeset::boxes::{Align, FootnoteBody, GlueKind, HBox, HItem, IndexTerm, Line, LineLink, LinkTarget, PlacedHBox},
};

/// 両端揃えで伸縮を配る順位の数（JLReq §3.8.3 / §3.8.4 の 3 段階）
const ADJUSTMENT_RANKS: usize = 3;

/// 水平リストを本文幅で行に分割する。両端揃えは段落全体最適（Knuth–Plass）、それ以外の揃えは貪欲法で組む。
/// 両端揃えでも、実現可能な分割が無いサブ段落は貪欲法の両端揃えで組む。両端揃えは行末の句読点を必要な場合だけ
/// `hang_room` の範囲でぶら下げる
///
/// `hang_room` は両端揃えの行末で句読点を利用幅の外へ出してよい幅。0 ならぶら下げない。両端揃え以外と貪欲法へ
/// 落ちたサブ段落は使わない。`order` は両端揃えの行の伸縮の配分だけが使う。行頭の始め括弧の前アキは、段落の最初の
/// 行が `paragraph_start`、それ以外の行（折返し後・強制改行後）が `line_start` に従う（[`open_bracket_lead`]）。
///
/// # Errors
///
/// 行の幅や伸縮の計算が長さの値域を超えたら [`LengthOverflow`] を返す。
pub(super) fn break_lines(
  items: &[HItem],
  text_width: Length,
  alignment: TextAlignment,
  hang_room: Length,
  order: AdjustmentOrder,
  paragraph_start: OpenBracketAki,
  line_start: OpenBracketAki,
) -> Result<Vec<Line>, LengthOverflow> {
  return if alignment == TextAlignment::Justify {
    knuth_plass::break_lines(items, text_width, hang_room, order, paragraph_start, line_start)
  } else {
    greedy::break_lines(items, text_width, alignment, &mut Vec::new(), order, paragraph_start, line_start)
  };
}

/// 行頭の始め括弧に足す前アキを返す
///
/// `items` は行頭の breakable glue を除いた行（の先頭から順に）。固定カーン（字下げ・明示アキ）と幅 0 の運搬マーカー（`LinkStart` /
/// `LinkEnd` / `IndexMark` / `Footnote`）を読み飛ばした最初のアイテムが始め括弧（[`HItem::OpenBracket`]）で、`aki` が
/// `Half` ならその括弧の前アキ、それ以外は 0。行はこの幅を利用幅から引いて組み、行全体をこの幅だけ右へずらす。行の
/// 選択（Knuth–Plass の可否）と配置が同じ値を使うので、選んだ行は配置でも利用幅に収まる。
fn open_bracket_lead<'a>(items: impl IntoIterator<Item = &'a HItem>, aki: OpenBracketAki) -> Length {
  if aki == OpenBracketAki::None {
    return Length::ZERO;
  }
  let first = items.into_iter().find(|item| {
    return !matches!(
      item,
      HItem::Kern(_) | HItem::LinkStart(_) | HItem::LinkEnd | HItem::IndexMark(_) | HItem::Footnote(_)
    );
  });
  return match first {
    Some(HItem::OpenBracket { lead, .. }) => *lead,
    _ => Length::ZERO,
  };
}

/// 両端揃えの行で glue の伸縮を配る順序
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdjustmentOrder {
  /// 全 glue に 1 つの調整比を掛ける（約物アキ調整を切った文書）
  Uniform,
  /// 種類の順位ごとに、前の順位を使い切ってから次の順位へ配る（JLReq §3.8.3 / §3.8.4）
  ByKind,
}

/// 両端揃えの 1 行の glue に配る調整（順位ごとの比。0 以上 1 以下）
#[derive(Debug, Clone, Copy, PartialEq)]
enum Adjustment {
  /// 伸縮しない（最終行・余りなし）
  Natural,
  /// 伸長
  Stretch([f64; ADJUSTMENT_RANKS]),
  /// 収縮
  Shrink([f64; ADJUSTMENT_RANKS]),
}

impl Adjustment {
  /// 種類 `kind`・伸長能力 `stretch`・収縮能力 `shrink` の glue の自然幅に足す量（収縮は負）
  ///
  /// `Uniform` では全 glue が順位 0 の比を受けるので、伸長は `stretch × 比`・収縮は `shrink × (−比)` と、
  /// 1 つの調整比を掛ける演算と同じ値になる。
  ///
  /// # Errors
  ///
  /// 積が長さの値域を超えたら [`LengthOverflow`]。
  fn delta(
    self,
    order: AdjustmentOrder,
    kind: GlueKind,
    stretch: Length,
    shrink: Length,
  ) -> Result<Length, LengthOverflow> {
    return match self {
      Adjustment::Natural => Ok(Length::ZERO),
      Adjustment::Stretch(ratios) => stretch.try_scale(ratios[stretch_rank(order, kind)]),
      Adjustment::Shrink(ratios) => shrink.try_scale(-ratios[shrink_rank(order, kind)]),
    };
  }
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
///
/// 合計は順位の割り当てによらない（[`rank_metrics`] の全順位の和）ので、行の可否をこの合計で決めると、配置
/// （[`glue_adjustment`]）が能力を順位ごとに使い切って同じ幅に揃えられる行と一致する。
fn glue_metrics(items: &[&HItem]) -> Result<(Length, Length, Length), LengthOverflow> {
  let (natural, stretch, shrink) = rank_metrics(items, AdjustmentOrder::Uniform)?;
  return Ok((natural, stretch[0], shrink[0]));
}

/// 行内アイテムの自然幅合計と、`order` の順位ごとの伸長能力・収縮能力の合計を求める（`FlushRight` は数えない）
fn rank_metrics(
  items: &[&HItem],
  order: AdjustmentOrder,
) -> Result<(Length, [Length; ADJUSTMENT_RANKS], [Length; ADJUSTMENT_RANKS]), LengthOverflow> {
  let mut natural = Length::ZERO;
  let mut stretch = [Length::ZERO; ADJUSTMENT_RANKS];
  let mut shrink = [Length::ZERO; ADJUSTMENT_RANKS];
  for item in items {
    match item {
      HItem::FlushRight(_) => {},
      HItem::Glue {
        natural: n,
        stretch: s,
        shrink: sh,
        kind,
        ..
      } => {
        natural = natural.try_add(*n)?;
        let stretch_at = &mut stretch[stretch_rank(order, *kind)];
        *stretch_at = stretch_at.try_add(*s)?;
        let shrink_at = &mut shrink[shrink_rank(order, *kind)];
        *shrink_at = shrink_at.try_add(*sh)?;
      },
      other => natural = natural.try_add(other.natural_width())?,
    }
  }
  return Ok((natural, stretch, shrink));
}

/// 種類 `kind` の glue が伸長する順位（`Uniform` は全種類 0）
///
/// 伸長能力を持たない種類（約物アキ）は最後の順位に置く。能力 0 なので配分は変わらない。
fn stretch_rank(order: AdjustmentOrder, kind: GlueKind) -> usize {
  return match order {
    AdjustmentOrder::Uniform => 0,
    AdjustmentOrder::ByKind => match kind {
      GlueKind::WordSpace => 0,
      GlueKind::JaLatinAki => 1,
      GlueKind::InterCharacter | GlueKind::MiddleDotAki | GlueKind::PunctuationAki => 2,
    },
  };
}

/// 種類 `kind` の glue が収縮する順位（`Uniform` は全種類 0）
///
/// 収縮能力を持たない種類（和欧文間アキ・和文字間）は最後の順位に置く。能力 0 なので配分は変わらない。
fn shrink_rank(order: AdjustmentOrder, kind: GlueKind) -> usize {
  return match order {
    AdjustmentOrder::Uniform => 0,
    AdjustmentOrder::ByKind => match kind {
      GlueKind::WordSpace => 0,
      GlueKind::MiddleDotAki => 1,
      GlueKind::PunctuationAki | GlueKind::JaLatinAki | GlueKind::InterCharacter => 2,
    },
  };
}

/// 必要量 `need` を順位 0 から順に能力 `capacities` へ割り当て、順位ごとの比（0 以上 1 以下）を返す
///
/// 前の順位を比 1 で使い切ってから次の順位へ進み、残りを受け切る順位は 残り ÷ 能力、その後ろは 0。能力を全部
/// 使っても足りなければ全順位が 1 で止まる。全能力が順位 0 にあれば `need ÷ 能力` を 1 で頭打ちにした 1 つの比になる。
fn fill_ranks(need: Length, capacities: [Length; ADJUSTMENT_RANKS]) -> [f64; ADJUSTMENT_RANKS] {
  let mut remaining = need;
  return capacities.map(|capacity| {
    if !remaining.is_positive() || !capacity.is_positive() {
      return 0.0;
    }
    if remaining <= capacity {
      let ratio = remaining.ratio(capacity);
      remaining = Length::ZERO;
      return ratio;
    }
    remaining = remaining.try_sub(capacity).expect("残りは能力より大きい正の値なので、差は値域内");
    return 1.0;
  });
}

/// 利用幅 `available` に対する行の余り幅を、`order` の順位で glue に配る調整を求める（両端揃えの配分）
fn glue_adjustment(items: &[&HItem], available: Length, order: AdjustmentOrder) -> Result<Adjustment, LengthOverflow> {
  let (natural, stretch, shrink) = rank_metrics(items, order)?;
  let leftover = available.try_sub(natural)?;
  return Ok(match leftover.cmp(&Length::ZERO) {
    Ordering::Greater => Adjustment::Stretch(fill_ranks(leftover, stretch)),
    Ordering::Less => Adjustment::Shrink(fill_ranks(-leftover, shrink)),
    Ordering::Equal => Adjustment::Natural,
  });
}

/// アイテム列から 1 行を組み立てる（位置確定）
///
/// `alignment` の水平ずらしまで済ませる。右寄せ末尾ボックス（`HItem::FlushRight`）はずらさず利用可能幅の
/// 右端に置き、本文はその幅を除いた残りの中で寄せる。本文の幅は [`Line::width`] と同じ「箱の右端の最大」で測る
/// （行末の Kern 等の空白は寄せる幅に含めない）。`end` がハイフンなら行内アイテムの直後にハイフンを置き、
/// ぶら下げなら句読点を除いた本文が利用可能幅に揃うよう伸縮して [`Line::hanging`] に句読点の幅を入れる。
/// 両端揃えの非最終行は余り幅を `order` の順位で glue に配る。
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
  order: AdjustmentOrder,
) -> Result<Line, LengthOverflow> {
  let items = trim_trailing_glue(items);

  // 両端揃えの配分。最終行は伸縮しない。
  // 行末ハイフンぶんを差し引き、ぶら下げる句読点ぶんを足した幅に対して配分する（ハイフン込み・句読点抜きで右端に揃える）
  let (trailing_hyphen, hanging) = match end {
    LineEnd::Plain => (None, Length::ZERO),
    LineEnd::Hyphen(hyphen) => (Some(hyphen), Length::ZERO),
    LineEnd::Hang(comma) => (None, comma.width),
  };
  let hyphen_width = trailing_hyphen.map_or(Length::ZERO, |hyphen| return hyphen.width);
  let adjustment = if alignment == TextAlignment::Justify && !is_last {
    glue_adjustment(items, available.try_sub(hyphen_width)?.try_add(hanging)?, order)?
  } else {
    Adjustment::Natural
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
      HItem::Box(hbox) | HItem::Comma(hbox) | HItem::OpenBracket { hbox, .. } => {
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
        kind,
        ..
      } => {
        x = x.try_add(natural.try_add(adjustment.delta(order, *kind, *stretch, *shrink)?)?)?;
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
      HItem::Box(_) | HItem::Comma(_) | HItem::OpenBracket { .. } | HItem::FlushRight(_) | HItem::Kern(_) => {
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
    typeset::boxes::{AnchorId, GlueKind, HBox, HBoxContent, HItem, IndexTerm, LinkTarget},
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
      kind: GlueKind::WordSpace,
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
      kind: GlueKind::WordSpace,
    };
  }

  /// テスト用の伸縮能力付き非 breakable glue（分割点にならず overflow 行を作れる）
  pub(super) fn non_breakable_stretch_glue() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: pt(2.5),
      shrink: pt(5.0 / 3.0),
      breakable: false,
      kind: GlueKind::WordSpace,
    };
  }

  /// テスト用の和文字間 glue（幅 0・伸長 0.5・収縮なし = フォントサイズ 10pt の字間相当）
  pub(super) fn cjk_glue() -> HItem {
    return HItem::Glue {
      natural: Length::ZERO,
      stretch: pt(0.5),
      shrink: Length::ZERO,
      breakable: true,
      kind: GlueKind::InterCharacter,
    };
  }

  /// テスト用の種類 `kind`・自然幅 `natural`・伸長 `stretch`・収縮 `shrink`（pt）の breakable glue
  pub(super) fn kind_glue(kind: GlueKind, natural: f32, stretch: f32, shrink: f32) -> HItem {
    return HItem::Glue {
      natural: pt(natural),
      stretch: pt(stretch),
      shrink: pt(shrink),
      breakable: true,
      kind,
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

  /// テスト用の始め括弧（幅 `width`・行頭で足せる前アキ `lead`・高さ 8・深さ 2）
  pub(super) fn open_bracket(width: f32, lead: f32) -> HItem {
    return HItem::OpenBracket {
      hbox: HBox {
        content: HBoxContent::Atom(Vec::new()),
        width: pt(width),
        height: pt(8.0),
        depth: pt(2.0),
      },
      lead: pt(lead),
    };
  }

  /// テスト用の句読点の後ろの約物アキ（幅 5・伸長なし・収縮 5・分割可）
  pub(super) fn comma_aki() -> HItem {
    return HItem::Glue {
      natural: pt(5.0),
      stretch: Length::ZERO,
      shrink: pt(5.0),
      breakable: true,
      kind: GlueKind::PunctuationAki,
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
  use proptest::prelude::*;

  use super::{
    ADJUSTMENT_RANKS, AdjustmentOrder, HItem, Length, Line, LineEnd, TextAlignment, break_lines, build_line,
    fill_ranks, greedy, knuth_plass, min_content_width, open_bracket_lead,
  };
  use crate::{
    style::OpenBracketAki,
    typeset::{
      boxes::{GlueKind, HBox},
      boxing::break_lines::test_support::{
        box_width, comma, comma_aki, discretionary, hang_items, kind_glue, math_break, non_breakable_stretch_glue,
        open_bracket, space_glue, stretch_glue, test_box,
      },
    },
  };

  /// 行頭の始め括弧の前アキなし
  const NONE: OpenBracketAki = OpenBracketAki::None;

  #[test]
  fn open_bracket_lead_skips_kerns_and_markers() {
    let kern = HItem::Kern(Length::pt(12.0));
    let open = open_bracket(6.0, 6.0);
    let letter = box_width(12.0);

    assert_eq!(open_bracket_lead([&kern, &HItem::LinkEnd, &open], OpenBracketAki::Half), Length::pt(6.0));
    assert_eq!(open_bracket_lead([&kern, &open], OpenBracketAki::None), Length::ZERO);
    assert_eq!(open_bracket_lead([&letter, &open], OpenBracketAki::Half), Length::ZERO);
  }

  /// 行ごとの先頭の箱の位置
  fn first_box_dx(lines: &[Line]) -> Vec<Length> { return lines.iter().map(|line| return line.boxes[0].dx).collect(); }

  #[test]
  fn line_start_lead_follows_the_key_of_each_line_start() {
    // 1 行目は段落先頭（自然幅 25 で、前アキ 5 を足すと幅 30 にちょうど収まる）、2 行目は折返し後、3 行目は明示改行後
    let items = vec![
      open_bracket(5.0, 5.0),
      box_width(10.0),
      non_breakable_stretch_glue(),
      box_width(5.0),
      comma_aki(),
      open_bracket(5.0, 5.0),
      box_width(10.0),
      HItem::ForcedBreak,
      open_bracket(5.0, 5.0),
      box_width(10.0),
    ];
    let half = OpenBracketAki::Half;
    let none = OpenBracketAki::None;

    for alignment in [TextAlignment::Justify, TextAlignment::Left] {
      let run = |paragraph_start, line_start| {
        let lines = break_lines(
          &items,
          Length::pt(30.0),
          alignment,
          Length::ZERO,
          AdjustmentOrder::ByKind,
          paragraph_start,
          line_start,
        )
        .expect("値域内");
        return first_box_dx(&lines);
      };

      assert_eq!(run(none, none), vec![Length::ZERO; 3], "{alignment:?}");
      assert_eq!(run(half, none), vec![Length::pt(5.0), Length::ZERO, Length::ZERO], "{alignment:?}");
      assert_eq!(run(none, half), vec![Length::ZERO, Length::pt(5.0), Length::pt(5.0)], "{alignment:?}");
    }
  }

  /// 収縮の順位を見る行: 語間・括弧読点のアキ・中点のアキを箱 10pt で挟む（自然幅 52.5pt）
  fn shrink_line() -> Vec<HItem> {
    return vec![
      box_width(10.0),
      kind_glue(GlueKind::WordSpace, 5.0, 2.5, 2.0),
      box_width(10.0),
      kind_glue(GlueKind::PunctuationAki, 5.0, 0.0, 5.0),
      box_width(10.0),
      kind_glue(GlueKind::MiddleDotAki, 2.5, 0.0, 2.5),
      box_width(10.0),
    ];
  }

  /// 伸長の順位を見る行: 語間・和欧文間アキ・和文字間を箱 10pt で挟む（自然幅 47.5pt）
  fn stretch_line() -> Vec<HItem> {
    return vec![
      box_width(10.0),
      kind_glue(GlueKind::WordSpace, 5.0, 2.5, 2.0),
      box_width(10.0),
      kind_glue(GlueKind::JaLatinAki, 2.5, 0.5, 0.0),
      box_width(10.0),
      kind_glue(GlueKind::InterCharacter, 0.0, 0.5, 0.0),
      box_width(10.0),
    ];
  }

  /// `items` を利用幅 `available`（pt）の非最終行として `order` で両端揃えに組む
  fn justify(items: &[HItem], available: f32, order: AdjustmentOrder) -> Line {
    let refs: Vec<&HItem> = items.iter().collect();
    return build_line(
      &refs,
      false,
      Length::pt(available),
      TextAlignment::Justify,
      &mut Vec::new(),
      LineEnd::Plain,
      order,
    )
    .expect("テストの寸法は値域内");
  }

  /// 組んだ行の各箱の `dx`
  fn place_sp(items: &[HItem], available: f32, order: AdjustmentOrder) -> Vec<Length> {
    return justify(items, available, order).boxes.iter().map(|placed| return placed.dx).collect();
  }

  /// 組んだ行の各箱の `dx`（pt）
  fn place(items: &[HItem], available: f32, order: AdjustmentOrder) -> Vec<f32> {
    return place_sp(items, available, order).iter().map(|dx| return dx.to_pt()).collect();
  }

  /// `actual` の各値が `expected` と差 1e-3 pt 未満で一致するか
  fn assert_dx(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len(), "actual: {actual:?}, expected: {expected:?}");
    for (a, e) in actual.iter().zip(expected) {
      assert!((a - e).abs() < 1e-3, "actual: {actual:?}, expected: {expected:?}");
    }
  }

  #[test]
  fn shrink_uses_word_spaces_before_punctuation_aki() {
    assert_dx(&place(&shrink_line(), 51.0, AdjustmentOrder::ByKind), &[0.0, 13.5, 28.5, 41.0]);
  }

  #[test]
  fn shrink_moves_to_middle_dot_aki_after_word_spaces() {
    assert_dx(&place(&shrink_line(), 49.5, AdjustmentOrder::ByKind), &[0.0, 13.0, 28.0, 39.5]);
  }

  #[test]
  fn shrink_reaches_punctuation_aki_last() {
    assert_dx(&place(&shrink_line(), 46.0, AdjustmentOrder::ByKind), &[0.0, 13.0, 26.0, 36.0]);
  }

  #[test]
  fn shrink_stops_at_the_limit() {
    // 必要量 12.5pt に対し能力は 9.5pt: 限界で止まり右端 43pt のまま溢れる
    assert_dx(&place(&shrink_line(), 40.0, AdjustmentOrder::ByKind), &[0.0, 13.0, 23.0, 33.0]);
  }

  #[test]
  fn stretch_uses_word_spaces_first() {
    assert_dx(&place(&stretch_line(), 49.5, AdjustmentOrder::ByKind), &[0.0, 17.0, 29.5, 39.5]);
  }

  #[test]
  fn stretch_moves_to_ja_latin_then_inter_character() {
    assert_dx(&place(&stretch_line(), 50.6, AdjustmentOrder::ByKind), &[0.0, 17.5, 30.5, 40.6]);
  }

  #[test]
  fn stretch_never_exceeds_the_capacity() {
    assert_dx(&place(&stretch_line(), 60.0, AdjustmentOrder::ByKind), &[0.0, 17.5, 30.5, 41.0]);
  }

  #[test]
  fn uniform_order_scales_every_glue_by_one_ratio() {
    // 必要量 6.5pt を能力 9.5pt の比（約 0.684）で全 glue に配る
    assert_dx(&place(&shrink_line(), 46.0, AdjustmentOrder::Uniform), &[0.0, 13.6316, 25.2105, 36.0]);
  }

  #[test]
  fn same_rank_glue_shares_shrink_in_proportion_to_capacity() {
    let items = vec![
      box_width(10.0),
      kind_glue(GlueKind::PunctuationAki, 5.0, 0.0, 5.0),
      box_width(10.0),
      kind_glue(GlueKind::PunctuationAki, 2.5, 0.0, 2.5),
      box_width(10.0),
    ];

    assert_dx(&place(&items, 34.5, AdjustmentOrder::ByKind), &[0.0, 13.0, 24.5]);
  }

  #[test]
  fn single_rank_order_matches_uniform_exactly() {
    // 語間だけの行: ByKind と Uniform の dx が sp 単位で一致する（欧文だけの段落の出力不変）
    let items = vec![
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
      stretch_glue(),
      box_width(10.0),
    ];

    for available in [30.0, 31.0, 36.0, 40.0] {
      assert_eq!(
        place_sp(&items, available, AdjustmentOrder::ByKind),
        place_sp(&items, available, AdjustmentOrder::Uniform),
        "{available}"
      );
    }
  }

  #[test]
  fn hanging_comma_is_not_absorbed_by_prioritized_shrink() {
    let hung = comma(5.0);
    let HItem::Comma(comma_box) = &hung else {
      panic!("句読点の箱を期待");
    };
    let comma_box: &HBox = comma_box;
    let items = [
      box_width(10.0),
      kind_glue(GlueKind::WordSpace, 5.0, 2.5, 2.0),
      box_width(10.0),
      hung.clone(),
    ];
    let refs: Vec<&HItem> = items.iter().collect();

    let line = build_line(
      &refs,
      false,
      Length::pt(25.0),
      TextAlignment::Justify,
      &mut Vec::new(),
      LineEnd::Hang(comma_box),
      AdjustmentOrder::ByKind,
    )
    .expect("テストの寸法は値域内");

    let dx: Vec<f32> = line.boxes.iter().map(|placed| return placed.dx.to_pt()).collect();
    assert_dx(&dx, &[0.0, 15.0, 25.0]);
    assert_eq!(line.hanging, Length::pt(5.0));
  }

  #[test]
  fn last_line_is_not_adjusted_in_either_order() {
    for order in [AdjustmentOrder::Uniform, AdjustmentOrder::ByKind] {
      for (items, available) in [(shrink_line(), 46.0), (stretch_line(), 60.0)] {
        let refs: Vec<&HItem> = items.iter().collect();

        let line = build_line(
          &refs,
          true,
          Length::pt(available),
          TextAlignment::Justify,
          &mut Vec::new(),
          LineEnd::Plain,
          order,
        )
        .expect("テストの寸法は値域内");

        // 期待値は自然幅で左から並べた箱の位置
        let mut x = 0.0f32;
        let mut expected = Vec::new();
        for item in &items {
          if matches!(item, HItem::Box(_)) {
            expected.push(x);
          }
          x += item.natural_width().to_pt();
        }
        let dx: Vec<f32> = line.boxes.iter().map(|placed| return placed.dx.to_pt()).collect();
        assert_dx(&dx, &expected);
      }
    }
  }

  /// glue の種類（proptest の添字から引く）
  const KINDS: [GlueKind; 5] = [
    GlueKind::WordSpace,
    GlueKind::JaLatinAki,
    GlueKind::InterCharacter,
    GlueKind::MiddleDotAki,
    GlueKind::PunctuationAki,
  ];

  proptest! {
    #[test]
    fn fill_ranks_saturates_earlier_ranks_first(
      need_sp in 0i64..2_000_000,
      caps_sp in prop::array::uniform3(0i64..1_000_000),
    ) {
      let caps = caps_sp.map(Length::from_sp);

      let ratios = fill_ranks(Length::from_sp(need_sp), caps);

      for ratio in ratios {
        prop_assert!((0.0..=1.0).contains(&ratio), "{ratios:?}");
      }
      for later in 1..ADJUSTMENT_RANKS {
        if ratios[later] > 0.0 {
          for earlier in 0..later {
            if caps[earlier].is_positive() {
              prop_assert!((ratios[earlier] - 1.0).abs() < f64::EPSILON, "{ratios:?}");
            }
          }
        }
      }
      #[expect(clippy::cast_precision_loss, reason = "テストの能力は 2^53 sp 未満で f64 に正確に載る")]
      let used: f64 = ratios.iter().zip(caps_sp).map(|(ratio, cap)| return ratio * cap as f64).sum();
      let total: i64 = caps_sp.iter().sum();
      #[expect(clippy::cast_precision_loss, reason = "テストの必要量は 2^53 sp 未満で f64 に正確に載る")]
      let expected = need_sp.min(total) as f64;
      prop_assert!((used - expected).abs() <= 3.0, "used {used}, expected {expected}");
    }

    #[test]
    fn placed_line_fills_the_width_within_capacity(
      glues in prop::collection::vec((1.0f32..20.0, 0usize..5, 0.0f32..6.0, 0.0f32..3.0, 0.0f32..=1.0), 1..12),
      t in -1.0f64..=1.0,
    ) {
      let mut items = Vec::new();
      for &(box_w, kind, natural, stretch, shrink_frac) in &glues {
        items.push(box_width(box_w));
        items.push(kind_glue(KINDS[kind], natural, stretch, natural * shrink_frac));
      }
      items.push(box_width(10.0));
      let sum = |part: fn(&HItem) -> Length| return Length::try_sum(items.iter().map(part)).expect("値域内");
      let natural = sum(HItem::natural_width);
      let stretch = sum(|item| return if let HItem::Glue { stretch, .. } = item { *stretch } else { Length::ZERO });
      let shrink = sum(|item| return if let HItem::Glue { shrink, .. } = item { *shrink } else { Length::ZERO });
      let room = if t >= 0.0 { stretch } else { shrink };
      let available = natural + room.try_scale(t).expect("値域内");
      let refs: Vec<&HItem> = items.iter().collect();

      for order in [AdjustmentOrder::Uniform, AdjustmentOrder::ByKind] {
        let line = build_line(&refs, false, available, TextAlignment::Justify, &mut Vec::new(), LineEnd::Plain, order)
          .expect("値域内");

        let error = (line.width().expect("値域内") - available).abs();
        #[expect(clippy::cast_possible_wrap, reason = "glue は 12 個未満")]
        let tolerance = Length::from_sp(glues.len() as i64 + 1);
        prop_assert!(error <= tolerance, "{order:?}: error {error:?}");
      }
    }
  }

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
      let lines = break_lines(&items, Length::pt(27.0), alignment, Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE)
        .expect("テストの寸法は値域内");
      let greedy =
        greedy::break_lines(&items, Length::pt(27.0), alignment, &mut Vec::new(), AdjustmentOrder::Uniform, NONE, NONE)
          .expect("テストの寸法は値域内");

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

    let lines =
      break_lines(&items, Length::pt(25.0), TextAlignment::Justify, Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE)
        .expect("テストの寸法は値域内");
    let optimal =
      knuth_plass::break_lines(&items, Length::pt(25.0), Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE)
        .expect("テストの寸法は値域内");

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
        break_lines(&hang_items(), Length::pt(30.0), alignment, Length::pt(5.0), AdjustmentOrder::Uniform, NONE, NONE)
          .expect("テストの寸法は値域内");

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

    let lines = break_lines(&items, width, TextAlignment::Left, Length::ZERO, AdjustmentOrder::Uniform, NONE, NONE)
      .expect("テストの寸法は値域内");

    assert!(lines.iter().all(|line| return line.width().expect("値域内") <= width), "{lines:?}");
  }
}
