//! 水平リストの積み場所 — run をまたぐ境界の約物アキを解決しながらアイテム列を積む
//!
//! 約物の内蔵アキは、run の内側なら `text_run` が隣接グリフ対ごとに戻す。この module は、アイテム列の端どうし
//! （和文 run の端の約物と、数式・欧文・別スタイルのテキスト・コード・別の和文 run との間）を同じ
//! `boundary_glue` の規則で解決する。幅 0 の運搬マーカーと脚注マーカーの箱は透明で、左の字に付いたまま残る。
//! 境界のアキは右のアイテムの直前（末尾の `LinkStart` の連なりより前）に挟むので、アキの分割点でマーカーが左の字から
//! 離れず、リンク領域はアキを含まない。両側がインライン数式の境界（数式の字・アトムと数式内の文章の端の約物の間）は、
//! 数式の中の他のアキと同じく分割せず伸縮もしない固定の `Kern` にする — インライン数式の分割点は演算子の後の `MathBreak` だけ。

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, HItem},
    boxing::{self, Glue, yakumono::YakumonoClass},
  },
};

/// アイテム列の端が約物アキに対して持つ性質
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Edge {
  /// 空白・明示アキ・分割制御・強制改行・リストの端 — 隣の約物のアキを打ち消す
  #[default]
  Absorb,
  /// 約物アキを受ける字（和文の通常字・欧文・数式の字やアトム・コード）
  Letter {
    /// インライン数式の中の字か
    math: bool,
  },
  /// 内蔵アキを切り詰めた和文約物
  Punct {
    /// 約物クラス（`Normal` 以外）
    class: YakumonoClass,
    /// その字の 1em
    em: Length,
    /// インライン数式の中（数式内の文章の端）の約物か
    math: bool,
  },
}

impl Edge {
  /// 和文 run のグリフの実効約物クラスから、本文の端の性質を作る（通常字は `Letter`）
  #[must_use]
  pub(super) fn glyph(class: YakumonoClass, em: Length) -> Edge {
    if class == YakumonoClass::Normal {
      return Edge::Letter { math: false };
    }
    return Edge::Punct {
      class,
      em,
      math: false,
    };
  }

  /// インライン数式の中の端にする（`Absorb` はそのまま）
  #[must_use]
  pub(super) fn in_math(self) -> Edge {
    return match self {
      Edge::Absorb => Edge::Absorb,
      Edge::Letter { .. } => Edge::Letter { math: true },
      Edge::Punct { class, em, .. } => Edge::Punct {
        class,
        em,
        math: true,
      },
    };
  }

  /// インライン数式の中の端か
  fn is_math(self) -> bool {
    return match self {
      Edge::Absorb => false,
      Edge::Letter { math } | Edge::Punct { math, .. } => math,
    };
  }
}

/// 境界の約物アキを解決しながら積む水平リスト
#[derive(Debug, Default)]
pub(super) struct HList {
  /// 積んだアイテム
  items: Vec<HItem>,
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
    self.trailing = trailing;
  }

  /// 本文の字として箱を積む
  pub(super) fn push_box(&mut self, hbox: HBox) {
    let edge = Edge::Letter { math: false };
    self.push(edge, vec![HItem::Box(hbox)], edge);
  }

  /// インライン数式の字・アトムとして箱を積む
  pub(super) fn push_math_box(&mut self, hbox: HBox) {
    let edge = Edge::Letter { math: true };
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

  /// リストの外の相手 `after` との境界を解決して、積んだアイテム列を返す
  #[must_use]
  pub(super) fn finish(mut self, after: Edge) -> Vec<HItem> {
    self.resolve(after);
    return self.items;
  }

  /// 最後の不透明アイテムの右端と `right` の境界に約物アキを挟む
  fn resolve(&mut self, right: Edge) {
    let Some(aki) = aki_between(self.trailing, right) else {
      return;
    };
    // `LinkStart` は右の内容の側に属するので、アキはその連なりの前に置く
    let link_starts = self.items.iter().rev().take_while(|item| return matches!(item, HItem::LinkStart(_))).count();
    self.items.insert(self.items.len() - link_starts, aki);
  }
}

/// 端 `left` → `right` の境界に挟む約物アキを決める（約物が絡まない境界・`Absorb` が絡む境界は `None`）
///
/// 1em は約物の側の字のもの、両側とも約物なら大きい方。右が中点のときだけ分割できない（中点は行頭禁則）。
/// 両側がインライン数式の中なら、自然幅の固定 `Kern` にする。
fn aki_between(left: Edge, right: Edge) -> Option<HItem> {
  let (left_class, right_class, em) = match (left, right) {
    (
      Edge::Punct {
        class: left_class,
        em: left_em,
        ..
      },
      Edge::Punct {
        class: right_class,
        em: right_em,
        ..
      },
    ) => (left_class, right_class, left_em.max(right_em)),
    (Edge::Letter { .. }, Edge::Punct { class, em, .. }) => (YakumonoClass::Normal, class, em),
    (Edge::Punct { class, em, .. }, Edge::Letter { .. }) => (class, YakumonoClass::Normal, em),
    (Edge::Absorb, _) | (_, Edge::Absorb) | (Edge::Letter { .. }, Edge::Letter { .. }) => return None,
  };
  let glue: Glue = boxing::boundary_glue(left_class, right_class, em, right_class != YakumonoClass::MiddleDot)?;
  if left.is_math() && right.is_math() {
    return Some(HItem::Kern(glue.natural));
  }
  return Some(glue.into_item());
}

#[cfg(test)]
mod tests {
  use super::{Edge, HList};
  use crate::{
    length::Length,
    typeset::{
      boxes::{HBox, HBoxContent, HItem, IndexTerm, LinkTarget},
      boxing::yakumono::YakumonoClass,
    },
  };

  const EM: Length = Length::from_sp(10 * 65536);

  /// 本文の字の端
  const BODY_LETTER: Edge = Edge::Letter { math: false };

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

  /// 和文約物の端
  fn punct(class: YakumonoClass) -> Edge { return Edge::glyph(class, EM); }

  /// アイテム列を種類の名前で並べる（glue は自然幅と、分割できないとき `nobreak`）
  fn shape(items: &[HItem]) -> Vec<String> {
    return items
      .iter()
      .map(|item| match item {
        HItem::Box(_) => return "box".to_string(),
        HItem::Glue {
          natural, breakable, ..
        } => return format!("glue {natural}{}", if *breakable { "" } else { " nobreak" }),
        HItem::LinkStart(_) => return "link-start".to_string(),
        HItem::LinkEnd => return "link-end".to_string(),
        HItem::IndexMark(_) => return "index".to_string(),
        HItem::Kern(length) => return format!("kern {length}"),
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

    push_glyph(&mut list, punct(YakumonoClass::Open));

    let items = list.finish(Edge::Absorb);
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
    list.push(BODY_LETTER, vec![HItem::Box(letter())], punct(YakumonoClass::Comma));

    list.push_box(letter());

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "glue 5pt", "box"]);
  }

  #[test]
  fn glue_goes_after_left_markers_and_before_a_trailing_link_start() {
    let mut markers = HList::default();
    push_glyph(&mut markers, punct(YakumonoClass::Comma));
    markers.push_marker(index_mark());
    markers.push_marker(HItem::LinkEnd);
    let mut link = HList::default();
    push_glyph(&mut link, punct(YakumonoClass::Comma));
    link.push_marker(HItem::LinkStart(LinkTarget::External("u".to_string())));

    markers.push_box(letter());
    link.push_box(letter());

    assert_eq!(shape(&markers.finish(Edge::Absorb)), ["box", "index", "link-end", "glue 5pt", "box"]);
    assert_eq!(shape(&link.finish(Edge::Absorb)), ["box", "glue 5pt", "link-start", "box"]);
  }

  #[test]
  fn marker_box_keeps_the_trailing_edge() {
    let mut list = HList::default();
    list.push_box(letter());
    list.push_marker(HItem::Box(letter()));

    push_glyph(&mut list, punct(YakumonoClass::Open));

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "box", "glue 5pt", "box"]);
  }

  #[test]
  fn absorbing_items_and_list_edges_cancel_the_aki() {
    let mut spaced = HList::default();
    push_glyph(&mut spaced, punct(YakumonoClass::Comma));
    spaced.push_absorbing(HItem::Glue {
      natural: Length::pt(3.0),
      stretch: Length::ZERO,
      shrink: Length::ZERO,
      breakable: true,
    });
    let mut edges = HList::default();

    spaced.push_box(letter());
    edges.push(punct(YakumonoClass::Open), vec![HItem::Box(letter())], punct(YakumonoClass::Comma));

    assert_eq!(shape(&spaced.finish(Edge::Absorb)), ["box", "glue 3pt", "box"], "空白はアキを打ち消す");
    assert_eq!(shape(&edges.finish(Edge::Absorb)), ["box"], "リストの端にはアキを足さない");
  }

  #[test]
  fn punct_pair_takes_the_larger_em() {
    let mut list = HList::default();
    list.push(BODY_LETTER, vec![HItem::Box(letter())], punct(YakumonoClass::Close));

    list.push(Edge::glyph(YakumonoClass::Open, EM * 2.0), vec![HItem::Box(letter())], BODY_LETTER);

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "glue 10pt", "box"], "」「 は二分 1 つ・大きい方の 1em");
  }

  #[test]
  fn middle_dot_on_the_right_is_not_breakable() {
    let mut list = HList::default();
    list.push_box(letter());

    push_glyph(&mut list, punct(YakumonoClass::MiddleDot));
    list.push_box(letter());

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "glue 2.5pt nobreak", "box", "glue 2.5pt", "box"]);
  }

  #[test]
  fn letter_pair_inserts_nothing() {
    let mut list = HList::default();
    list.push_box(letter());

    list.push_box(letter());

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "box"]);
  }

  #[test]
  fn empty_push_keeps_the_trailing_edge() {
    let mut list = HList::default();
    list.push(BODY_LETTER, vec![HItem::Box(letter())], punct(YakumonoClass::Comma));

    list.push(Edge::Absorb, Vec::new(), Edge::Absorb);
    list.push_box(letter());

    assert_eq!(shape(&list.finish(Edge::Absorb)), ["box", "glue 5pt", "box"]);
  }

  #[test]
  fn math_pair_with_prose_punct_yields_a_fixed_kern() {
    let mut inside = HList::default();
    inside.push_math_box(letter());
    let mut boundary = HList::default();
    push_glyph(&mut boundary, punct(YakumonoClass::Comma));

    push_glyph(&mut inside, punct(YakumonoClass::Open).in_math());
    push_glyph(&mut boundary, punct(YakumonoClass::Open).in_math());

    assert_eq!(shape(&inside.finish(Edge::Absorb)), ["box", "kern 5pt", "box"], "数式の中は分割も伸縮もしない");
    assert_eq!(shape(&boundary.finish(Edge::Absorb)), ["box", "glue 5pt", "box"], "数式の境界は本文と同じ glue");
  }

  #[test]
  fn edges_report_the_first_and_last_opaque_edges() {
    let mut list = HList::default();
    list.push_marker(index_mark());
    list.push_absorbing(HItem::Kern(Length::pt(1.0)));
    push_glyph(&mut list, punct(YakumonoClass::Close));

    assert_eq!(
      list.edges(),
      (Edge::Absorb, punct(YakumonoClass::Close)),
      "先頭の空白は Absorb、末尾は最後の列の右端"
    );
    assert_eq!(HList::default().edges(), (Edge::Absorb, Edge::Absorb));
  }
}
