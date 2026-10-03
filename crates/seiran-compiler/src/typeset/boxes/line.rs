//! 行分割の出力。

use crate::{
  length::Length,
  typeset::boxes::{
    hitem::{IndexTerm, MeasuredFootnote, PlacedHBox},
    link::LinkTarget,
    page::PlacedLink,
  },
};

/// 行分割で確定した 1 行
///
/// `height` / `depth` は行内ボックスの `dy ± height/depth` の最大値。
#[derive(Debug, Clone)]
pub(crate) struct Line {
  /// 行内の配置済みボックス（左から順）
  pub boxes: Vec<PlacedHBox>,
  /// ベースラインから上の高さ
  pub height: Length,
  /// ベースラインから下の深さ（正値）
  pub depth: Length,
  /// この行に含まれるクリック可能なリンク領域（機構 B・行頭からの水平範囲）
  ///
  /// 1 つのリンクが折り返しをまたぐ場合は行ごとに 1 つの矩形へ分割される。
  pub links: Vec<LineLink>,
  /// この行に含まれる脚注（出現順）
  pub footnotes: Vec<MeasuredFootnote>,
  /// この行に含まれる索引語（`\index{語}`、出現順）
  pub index_marks: Vec<IndexTerm>,
}

impl Line {
  /// 行の実効幅（最も右へ伸びるボックスの右端）
  ///
  /// `boxes` は左から順に積まれるが、両端揃えの伸縮や約物の負アキで最後の箱が最右とは限らないため、
  /// 右端の最大値を取る。
  #[must_use]
  pub(crate) fn width(&self) -> Length {
    return self.boxes.iter().map(|placed| return placed.dx + placed.hbox.width).fold(Length::ZERO, Length::max);
  }

  /// 行内の水平位置（ボックスとクリック矩形）をまとめて `dx` だけ右へずらす
  ///
  /// `Line` の x はすべて行頭（段左端）からの相対値なので、インデント・揃えオフセット・
  /// 段オフセットのように「行の着地位置が決まってから足す量」はこのメソッドで一括して加える。
  /// x を持つのは `boxes` と `links` の 2 つだけで、`index_marks` / `footnotes` は座標を持たない。
  pub(crate) fn shift_x(&mut self, dx: Length) {
    if dx == Length::ZERO {
      return;
    }
    for placed in &mut self.boxes {
      placed.dx += dx;
    }
    for link in &mut self.links {
      link.x0 += dx;
      link.x1 += dx;
    }
  }
}

/// 水平 1 行内のリンク領域（クリック矩形の水平範囲）
///
/// `x0` / `x1` は基準点からの水平オフセット。基準点は持ち主で決まり、[`Line::links`] なら行頭
/// （着地する段の左端）、表行（`collect_row_links` の戻り値）なら表の左端。縦範囲は持たず、
/// 確定座標への展開時（[`LineLink::place`]）に呼び出し側が与える。
#[derive(Debug, Clone)]
pub(crate) struct LineLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 領域左端の基準点からの水平オフセット
  pub x0: Length,
  /// 領域右端の基準点からの水平オフセット
  pub x1: Length,
}

impl LineLink {
  /// 基準点を `dx` に置いたときの確定矩形を返す。縦範囲は `top` から `height`
  ///
  /// 退化矩形（`x1 <= x0`）は描画しないので `None`。
  #[must_use]
  pub(crate) fn place(&self, dx: Length, top: Length, height: Length) -> Option<PlacedLink> {
    if self.x1 <= self.x0 {
      return None;
    }
    return Some(PlacedLink {
      target: self.target.clone(),
      x: dx + self.x0,
      y: top,
      width: self.x1 - self.x0,
      height,
    });
  }
}
