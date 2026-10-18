//! 水平リストの積み場所 — run 境界の約物アキ・和欧文間アキを解決しながらアイテム列を積む
//!
//! 約物の内蔵アキは、run の内側なら `text_run` が隣接グリフ対ごとに戻す。この module は、アイテム列の端どうし
//! （和文 run の端の約物と、数式・欧文・別スタイルのテキスト・コード・別の和文 run との間）を同じ
//! `boundary_glue` の規則で解決する。アキで分割できるかは run の内側と同じく、境界の 2 字を UAX #14 で読んで決める
//! （欧文の閉じ約物の前・開き約物の後ろ・中点の前では分割しない）。幅 0 の運搬マーカーと脚注マーカーの箱は透明で、
//! 左の字に付いたまま残る。境界のアキは右のアイテムの直前（後ろに続く透明マーカーに閉じていないリンクの `LinkStart` があればその前）に挟むので、アキの分割点で
//! マーカーが左の字から離れず、リンク領域はアキを含まない。両側が数式の中の境界（数式の字・アトムと数式内の文章の端の
//! 約物の間）は、数式の中の他のアキと同じく分割せず伸縮もしない固定の `Kern` にする — インライン数式の分割点は演算子の
//! 後の `MathBreak` だけ。
//!
//! 和欧文間アキ（四分・分割しない）は、本文テキストの和文字と欧文字（どちらも英数字）が接する run 境界に、和文側の
//! 1em で入れる。1 つのテキストの中のスクリプト境界も run 境界なので、和欧文間アキはここ 1 箇所で決まる。文字を
//! 持たない箱（数式・コード）と数式の中の字は受けない。

use tracing::trace;

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, HItem},
    boxing::{
      self, Glue, break_opportunities,
      script::ScriptCategory,
      yakumono::{self, YakumonoClass},
    },
  },
};

/// 文字を持たない箱（数式の字・アトム・コード）の端を UAX #14 で読むときの代理の字
///
/// 欧文字（AL）として読むので、隣の約物の禁則（閉じ約物・中点の前、開き約物の後ろで分割しない）は欧文字の隣と同じになる。
pub(super) const BOX_EDGE_CHAR: char = 'x';

/// アイテム列の端が約物アキに対して持つ性質
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Edge {
  /// 空白・明示アキ・分割制御・強制改行・リストの端 — 隣の約物のアキを打ち消す
  #[default]
  Absorb,
  /// 約物アキを受ける字（和文の通常字・欧文・数式の字やアトム・コード）
  Letter {
    /// 端の字（文字を持たない箱は [`BOX_EDGE_CHAR`]）
    ch: char,
    /// 数式の中の字か
    math: bool,
    /// 和欧文間アキを受ける本文テキストの字なら（スクリプト区分, その字の 1em）。文字を持たない箱・数式の中の字・
    /// アキ調整を切った文書の字は `None`
    script: Option<(ScriptCategory, Length)>,
  },
  /// 内蔵アキを切り詰めた和文約物
  Punct {
    /// 約物の字（約物クラスが `Normal` 以外の字）
    ch: char,
    /// その字の 1em
    em: Length,
    /// 数式の中（数式内の文章の端）の約物か
    math: bool,
  },
}

impl Edge {
  /// 和欧文間アキを受けない本文の字の端（文字を持たない箱・アキ調整を切った文書の字）
  #[must_use]
  pub(super) fn letter(ch: char) -> Edge {
    return Edge::Letter {
      ch,
      math: false,
      script: None,
    };
  }

  /// 和欧文間アキを受ける本文テキストの字の端（`em` はその字の 1em）
  #[must_use]
  pub(super) fn text(ch: char, category: ScriptCategory, em: Length) -> Edge {
    return Edge::Letter {
      ch,
      math: false,
      script: Some((category, em)),
    };
  }

  /// 和文 run のグリフ `ch` とその実効約物クラス `class` から、本文の端の性質を作る（`Normal` は和文の字の端）
  ///
  /// `class` は箱を詰めたかどうかを決めたクラス — 詰めていない約物（半角グリフ・複数グリフのクラスタ）は字の端になる。
  /// 約物を詰める和文 run はアキ調整のある文書にしか無いので、字の端は和欧文間アキを受ける。
  #[must_use]
  pub(super) fn glyph(ch: char, class: YakumonoClass, em: Length) -> Edge {
    if class == YakumonoClass::Normal {
      return Edge::text(ch, ScriptCategory::Japanese, em);
    }
    return Edge::Punct {
      ch,
      em,
      math: false,
    };
  }

  /// 数式の中の端にする（`Absorb` はそのまま。数式の中の字は和欧文間アキを受けない）
  #[must_use]
  pub(super) fn in_math(self) -> Edge {
    return match self {
      Edge::Absorb => Edge::Absorb,
      Edge::Letter { ch, .. } => Edge::Letter {
        ch,
        math: true,
        script: None,
      },
      Edge::Punct { ch, em, .. } => Edge::Punct { ch, em, math: true },
    };
  }
}

/// 境界の約物アキを解決しながら積む水平リスト
#[derive(Debug, Default)]
pub(super) struct HList {
  /// 積んだアイテム
  items: Vec<HItem>,
  /// 最後に積んだ不透明アイテムの直後の位置（ここから後ろは透明マーカーだけ）
  opaque_end: usize,
  /// 最初に積んだ不透明アイテムの左端（まだ積んでいなければ `None`）
  leading: Option<Edge>,
  /// 最後に積んだ不透明アイテムの右端（まだ積んでいなければリストの外の相手）
  trailing: Edge,
}

impl HList {
  /// 左端 `leading`・右端 `trailing` のアイテム列を、手前との境界のアキを解決してから積む（空の列は何も変えない）
  pub(super) fn push(&mut self, leading: Edge, items: Vec<HItem>, trailing: Edge) {
    if items.is_empty() {
      return;
    }
    self.resolve(leading);
    self.leading.get_or_insert(leading);
    self.items.extend(items);
    self.opaque_end = self.items.len();
    self.trailing = trailing;
  }

  /// 本文の文字を持たない箱（コード）として積む
  pub(super) fn push_box(&mut self, hbox: HBox) {
    let edge = Edge::letter(BOX_EDGE_CHAR);
    self.push(edge, vec![HItem::Box(hbox)], edge);
  }

  /// 数式の字・アトムとして箱を積む
  pub(super) fn push_math_box(&mut self, hbox: HBox) {
    let edge = Edge::letter(BOX_EDGE_CHAR).in_math();
    self.push(edge, vec![HItem::Box(hbox)], edge);
  }

  /// 隣の約物のアキを打ち消すアイテム（空白・明示アキ・分割制御）を積む
  pub(super) fn push_absorbing(&mut self, item: HItem) { self.push(Edge::Absorb, vec![item], Edge::Absorb); }

  /// 約物アキの解決に関わらない透明なアイテム（運搬マーカー・脚注マーカーの箱）を積む
  pub(super) fn push_marker(&mut self, item: HItem) { self.items.push(item); }

  /// アイテムを 1 つも積んでいないか
  #[must_use]
  pub(super) fn is_empty(&self) -> bool { return self.items.is_empty(); }

  /// 積んだアイテム列の（左端, 右端）の性質（不透明アイテムが無ければ両端とも `Absorb`）
  #[must_use]
  pub(super) fn edges(&self) -> (Edge, Edge) { return (self.leading.unwrap_or(Edge::Absorb), self.trailing); }

  /// 積んだアイテム列を返す（リストの端はアキを打ち消すので、末尾の約物にアキは足さない）
  #[must_use]
  pub(super) fn into_items(self) -> Vec<HItem> { return self.items; }

  /// 最後の不透明アイテムの右端と `right` の境界に約物アキを挟む
  fn resolve(&mut self, right: Edge) {
    let Some(aki) = aki_between(self.trailing, right) else {
      return;
    };
    // 後ろに続く透明マーカーのうち、閉じていないリンクの `LinkStart` は右の内容の側に属するので、アキはその前に置く
    // （`LinkEnd` まで閉じたリンク — 脚注マーカーなど — は左の字に付く）
    let tail = &self.items[self.opaque_end..];
    let open_from = tail.iter().rposition(|item| return matches!(item, HItem::LinkEnd)).map_or(0, |end| return end + 1);
    let at = tail[open_from..]
      .iter()
      .position(|item| return matches!(item, HItem::LinkStart(_)))
      .map_or(self.items.len(), |offset| return self.opaque_end + open_from + offset);
    self.items.insert(at, aki);
  }
}

/// 端 `left` → `right` の境界に挟む約物アキ・和欧文間アキを決める（どちらも絡まない境界・`Absorb` が絡む境界は `None`）
///
/// 約物アキの 1em は約物の側の字のもの、両側とも約物なら大きい方。分割できるのは境界の 2 字の間に UAX #14 の分割機会が
/// あるときだけ。両側が数式の中なら、自然幅の固定 `Kern` にする。和欧文間アキは両側とも字の端のときだけで、約物の字は
/// 英数字でないので約物アキと同じ境界には重ならない。
pub(super) fn aki_between(left: Edge, right: Edge) -> Option<HItem> {
  if let (
    Edge::Letter {
      ch: left_char,
      script: Some((left_category, left_em)),
      ..
    },
    Edge::Letter {
      ch: right_char,
      script: Some((right_category, right_em)),
      ..
    },
  ) = (left, right)
  {
    return ja_latin_between((left_char, left_category, left_em), (right_char, right_category, right_em));
  }
  let (left_ch, left_math, left_em) = edge_parts(left)?;
  let (right_ch, right_math, right_em) = edge_parts(right)?;
  let em = match (left_em, right_em) {
    (Some(left_em), Some(right_em)) => left_em.max(right_em),
    (Some(em), None) | (None, Some(em)) => em,
    (None, None) => return None,
  };
  let class_of = |ch: char, em: Option<Length>| -> YakumonoClass {
    if em.is_none() {
      return YakumonoClass::Normal;
    }
    let class = yakumono::classify(ch);
    if class == YakumonoClass::Normal {
      unreachable!("Edge::Punct は Edge::glyph が約物クラスの字にだけ作る（in_math も字を変えない）");
    }
    return class;
  };
  let pair = format!("{left_ch}{right_ch}");
  let breakable = break_opportunities::break_opportunities(&pair, None)
    .iter()
    .any(|point| return point.byte == left_ch.len_utf8());
  let glue: Glue = boxing::boundary_glue(class_of(left_ch, left_em), class_of(right_ch, right_em), em, breakable)?;
  if left_math && right_math {
    return Some(HItem::Kern(glue.natural));
  }
  return Some(glue.into_item());
}

/// 本文テキストの字の端どうし（それぞれ（字, スクリプト区分, 1em））の境界に挟む和欧文間アキ
///
/// 和文字と欧文字（どちらも英数字）が接するときだけ、和文側の 1em の四分アキ（分割しない）を返す。
fn ja_latin_between(
  (left_char, left_category, left_em): (char, ScriptCategory, Length),
  (right_char, right_category, right_em): (char, ScriptCategory, Length),
) -> Option<HItem> {
  if !boxing::is_ja_latin_letter_boundary(left_category, left_char, right_category, right_char) {
    return None;
  }
  let em = if left_category == ScriptCategory::Japanese {
    left_em
  } else {
    right_em
  };
  let glue = boxing::ja_latin_aki(em);
  trace!(
    left_char = ?left_char,
    right_char = ?right_char,
    left_category = ?left_category,
    right_category = ?right_category,
    natural_pt = %glue.natural.to_pt(),
    stretch_pt = %glue.stretch.to_pt(),
    shrink_pt = %glue.shrink.to_pt(),
    "和欧文間アキを挿入"
  );
  return Some(glue.into_item());
}

/// 端の（字, 数式の中か, 約物ならその 1em）（`Absorb` は `None`）
fn edge_parts(edge: Edge) -> Option<(char, bool, Option<Length>)> {
  return match edge {
    Edge::Absorb => None,
    Edge::Letter { ch, math, .. } => Some((ch, math, None)),
    Edge::Punct { ch, em, math } => Some((ch, math, Some(em))),
  };
}

#[cfg(test)]
mod tests {
  use super::{BOX_EDGE_CHAR, Edge, HList};
  use crate::{
    length::Length,
    typeset::{
      boxes::{HBox, HBoxContent, HItem, IndexTerm, LinkTarget},
      boxing::{script::ScriptCategory, yakumono},
    },
  };

  const EM: Length = Length::from_sp(10 * 65536);

  /// 幅 10pt の空の箱
  fn letter() -> HBox {
    return HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: Length::pt(10.0),
      height: Length::ZERO,
      depth: Length::ZERO,
    };
  }

  /// 字 1 つぶんの列（端の性質は `edge`）を積む
  fn push_glyph(list: &mut HList, edge: Edge) { list.push(edge, vec![HItem::Box(letter())], edge); }

  /// 本文の字 `ch` の端（約物なら詰めた約物の端）
  fn glyph(ch: char) -> Edge { return Edge::glyph(ch, yakumono::classify(ch), EM); }

  /// 本文の和文字 `ch` の端
  fn ja(ch: char) -> Edge { return Edge::text(ch, ScriptCategory::Japanese, EM); }

  /// 本文の欧文字 `ch` の端
  fn latin(ch: char) -> Edge { return Edge::text(ch, ScriptCategory::Latin, EM); }

  /// アイテム列を種類の名前で並べる（glue は自然幅と、分割できないとき `nobreak`）
  fn shape(items: &[HItem]) -> Vec<String> {
    return items
      .iter()
      .map(|item| match item {
        HItem::Box(_) => return "box".to_string(),
        HItem::Glue {
          natural, breakable, ..
        } => return format!("glue {natural}{}", if *breakable { "" } else { " nobreak" }),
        HItem::Kern(length) => return format!("kern {length}"),
        HItem::LinkStart(_) => return "link-start".to_string(),
        HItem::LinkEnd => return "link-end".to_string(),
        HItem::IndexMark(_) => return "index".to_string(),
        other => return format!("{other:?}"),
      })
      .collect();
  }

  /// 索引語マーカー
  fn index_mark() -> HItem {
    return HItem::IndexMark(IndexTerm {
      word: "k".to_string(),
      reading: None,
    });
  }

  #[test]
  fn letter_then_punct_inserts_the_front_aki() {
    let mut list = HList::default();
    list.push_box(letter());

    push_glyph(&mut list, glyph('（'));

    let items = list.into_items();
    assert_eq!(shape(&items), ["box", "glue 5pt", "box"]);
    let HItem::Glue {
      stretch, shrink, ..
    } = items[1]
    else {
      panic!("Glue を期待");
    };
    assert_eq!((stretch, shrink), (Length::ZERO, Length::pt(5.0)), "伸長なし・詰め代は二分");
  }

  #[test]
  fn punct_then_letter_is_resolved_by_the_next_push() {
    let mut list = HList::default();
    list.push(glyph('値'), vec![HItem::Box(letter())], glyph('、'));

    list.push_box(letter());

    assert_eq!(shape(&list.into_items()), ["box", "glue 5pt", "box"]);
  }

  #[test]
  fn glue_goes_after_left_markers_and_before_a_trailing_link_start() {
    let mut markers = HList::default();
    push_glyph(&mut markers, glyph('、'));
    markers.push_marker(index_mark());
    markers.push_marker(HItem::LinkEnd);
    let mut link = HList::default();
    push_glyph(&mut link, glyph('、'));
    link.push_marker(HItem::LinkStart(LinkTarget::External("u".to_string())));

    markers.push_box(letter());
    link.push_box(letter());

    assert_eq!(shape(&markers.into_items()), ["box", "index", "link-end", "glue 5pt", "box"]);
    assert_eq!(shape(&link.into_items()), ["box", "glue 5pt", "link-start", "box"]);
  }

  #[test]
  fn glue_goes_before_a_link_start_followed_by_other_markers() {
    let mut list = HList::default();
    push_glyph(&mut list, glyph('、'));
    list.push_marker(index_mark());
    list.push_marker(HItem::LinkStart(LinkTarget::External("u".to_string())));
    list.push_marker(index_mark());

    list.push_box(letter());

    assert_eq!(
      shape(&list.into_items()),
      ["box", "index", "glue 5pt", "link-start", "index", "box"],
      "リンクの中のマーカーがあってもアキはリンク領域の外"
    );
  }

  #[test]
  fn marker_box_keeps_the_trailing_edge() {
    let mut list = HList::default();
    list.push_box(letter());
    list.push_marker(HItem::Box(letter()));

    push_glyph(&mut list, glyph('（'));

    assert_eq!(shape(&list.into_items()), ["box", "box", "glue 5pt", "box"]);
  }

  #[test]
  fn absorbing_items_and_list_edges_cancel_the_aki() {
    let mut spaced = HList::default();
    push_glyph(&mut spaced, glyph('、'));
    spaced.push_absorbing(HItem::Glue {
      natural: Length::pt(3.0),
      stretch: Length::ZERO,
      shrink: Length::ZERO,
      breakable: true,
    });
    let mut edges = HList::default();

    spaced.push_box(letter());
    edges.push(glyph('（'), vec![HItem::Box(letter())], glyph('、'));

    assert_eq!(shape(&spaced.into_items()), ["box", "glue 3pt", "box"], "空白はアキを打ち消す");
    assert_eq!(shape(&edges.into_items()), ["box"], "リストの端にはアキを足さない");
  }

  #[test]
  fn punct_pair_takes_the_larger_em() {
    let mut list = HList::default();
    list.push(glyph('値'), vec![HItem::Box(letter())], glyph('」'));

    list.push(Edge::glyph('「', yakumono::classify('「'), EM * 2.0), vec![HItem::Box(letter())], glyph('値'));

    assert_eq!(shape(&list.into_items()), ["box", "glue 10pt", "box"], "」「 は二分 1 つ・大きい方の 1em");
  }

  #[test]
  fn breakability_follows_uax14_at_the_boundary() {
    let mut middle_dot = HList::default();
    middle_dot.push_box(letter());
    let mut latin_close = HList::default();
    push_glyph(&mut latin_close, glyph('」'));
    let mut latin_open = HList::default();
    push_glyph(&mut latin_open, Edge::letter('('));

    push_glyph(&mut middle_dot, glyph('・'));
    middle_dot.push_box(letter());
    push_glyph(&mut latin_close, Edge::letter(','));
    push_glyph(&mut latin_open, glyph('「'));

    assert_eq!(
      shape(&middle_dot.into_items()),
      ["box", "glue 2.5pt nobreak", "box", "glue 2.5pt", "box"],
      "中点の前では分割しない"
    );
    assert_eq!(
      shape(&latin_close.into_items()),
      ["box", "glue 5pt nobreak", "box"],
      "欧文の閉じ約物の前では分割しない"
    );
    assert_eq!(
      shape(&latin_open.into_items()),
      ["box", "glue 5pt nobreak", "box"],
      "欧文の開き約物の後ろでは分割しない"
    );
  }

  #[test]
  fn letter_pair_inserts_nothing() {
    let mut list = HList::default();
    list.push_box(letter());

    list.push_box(letter());

    assert_eq!(shape(&list.into_items()), ["box", "box"]);
  }

  #[test]
  fn empty_push_keeps_the_trailing_edge() {
    let mut list = HList::default();
    list.push(glyph('値'), vec![HItem::Box(letter())], glyph('、'));

    list.push(Edge::Absorb, Vec::new(), Edge::Absorb);
    list.push_box(letter());

    assert_eq!(shape(&list.into_items()), ["box", "glue 5pt", "box"]);
  }

  #[test]
  fn math_pair_with_prose_punct_yields_a_fixed_kern() {
    let mut inside = HList::default();
    inside.push_math_box(letter());
    let mut boundary = HList::default();
    push_glyph(&mut boundary, glyph('、'));

    push_glyph(&mut inside, glyph('（').in_math());
    push_glyph(&mut boundary, glyph('（').in_math());

    assert_eq!(shape(&inside.into_items()), ["box", "kern 5pt", "box"], "数式の中は分割も伸縮もしない");
    assert_eq!(shape(&boundary.into_items()), ["box", "glue 5pt", "box"], "数式の境界は本文と同じ glue");
  }

  #[test]
  fn edges_report_the_first_and_last_opaque_edges() {
    let mut list = HList::default();
    list.push_marker(index_mark());
    list.push_absorbing(HItem::Kern(Length::pt(1.0)));
    push_glyph(&mut list, glyph('）'));

    assert_eq!(list.edges(), (Edge::Absorb, glyph('）')), "先頭の空白は Absorb、末尾は最後の列の右端");
    assert_eq!(HList::default().edges(), (Edge::Absorb, Edge::Absorb));
    assert_eq!(Edge::glyph('（', yakumono::YakumonoClass::Normal, EM), ja('（'), "詰めない約物は和文の字の端");
    assert_eq!(
      Edge::letter(BOX_EDGE_CHAR).in_math(),
      Edge::Letter {
        ch: BOX_EDGE_CHAR,
        math: true,
        script: None
      }
    );
    assert_eq!(
      ja('本').in_math(),
      Edge::Letter {
        ch: '本',
        math: true,
        script: None
      },
      "数式の中の字は和欧文間アキを受けない"
    );
  }

  #[test]
  fn japanese_and_latin_letters_get_the_quarter_em_aki_once() {
    let mut forward = HList::default();
    push_glyph(&mut forward, ja('本'));
    let mut backward = HList::default();
    push_glyph(&mut backward, latin('C'));

    push_glyph(&mut forward, latin('A'));
    push_glyph(&mut backward, ja('語'));

    assert_eq!(shape(&forward.into_items()), ["box", "glue 2.5pt nobreak", "box"], "本→A は四分・分割しない");
    assert_eq!(shape(&backward.into_items()), ["box", "glue 2.5pt nobreak", "box"], "C→語 も四分");
  }

  #[test]
  fn ja_latin_aki_takes_the_em_of_the_japanese_side() {
    let mut list = HList::default();
    push_glyph(&mut list, Edge::text('A', ScriptCategory::Latin, EM * 2.0));

    push_glyph(&mut list, ja('語'));

    assert_eq!(shape(&list.into_items()), ["box", "glue 2.5pt nobreak", "box"], "和文側の 1em（10pt）の四分");
  }

  #[test]
  fn ja_latin_aki_needs_two_text_letters_of_different_scripts() {
    // 箱（コード・数式）・数式の中の字・同じ区分・英数字でない字の境界には入れない
    let cases = [
      (ja('本'), Edge::letter(BOX_EDGE_CHAR)),
      (ja('本'), Edge::letter(BOX_EDGE_CHAR).in_math()),
      (ja('本').in_math(), latin('A')),
      (ja('本'), ja('語')),
      (latin('A'), latin('B')),
      (ja('※'), latin('A')),
    ];
    for (left, right) in cases {
      let mut list = HList::default();
      push_glyph(&mut list, left);

      push_glyph(&mut list, right);

      assert_eq!(shape(&list.into_items()), ["box", "box"], "{left:?} → {right:?}");
    }
  }
}
