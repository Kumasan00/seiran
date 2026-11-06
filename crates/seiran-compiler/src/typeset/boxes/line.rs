//! 行分割の出力。

use crate::{
  length::{Length, LengthOverflow},
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
  ///
  /// # Errors
  ///
  /// いずれかのボックスの右端が長さの値域に収まらないとき [`LengthOverflow`]。
  pub(crate) fn width(&self) -> Result<Length, LengthOverflow> {
    return self
      .boxes
      .iter()
      .try_fold(Length::ZERO, |widest, placed| return Ok(widest.max(placed.dx.try_add(placed.hbox.width)?)));
  }

  /// 行内の水平位置（ボックスとクリック矩形）をまとめて `dx` だけ右へずらす
  ///
  /// `Line` の水平位置はすべて行頭（段左端）からの相対値なので、インデント・揃えオフセット・
  /// 段オフセットのように「行の着地位置が決まってから足す量」はこのメソッドで一括して加える。
  /// 水平位置を持つのは `boxes`（`dx`）と `links`（`x0` / `x1`）の 2 つだけで、`index_marks` / `footnotes` は座標を持たない。
  ///
  /// # Errors
  ///
  /// ずらした位置が長さの値域に収まらないとき [`LengthOverflow`]（その時点までのずらしは残る。呼び元は組版を止める）。
  pub(in crate::typeset) fn shift_x(&mut self, dx: Length) -> Result<(), LengthOverflow> {
    if dx == Length::ZERO {
      return Ok(());
    }
    for placed in &mut self.boxes {
      placed.dx = placed.dx.try_add(dx)?;
    }
    for link in &mut self.links {
      link.x0 = link.x0.try_add(dx)?;
      link.x1 = link.x1.try_add(dx)?;
    }
    return Ok(());
  }
}

/// 水平 1 行内のリンク領域（クリック矩形の水平範囲）
///
/// `x0` / `x1` は基準点からの水平オフセット。基準点は持ち主で決まり、[`Line::links`] なら行の水平基準
/// （行分割直後は行頭で、[`Line::shift_x`] が着地位置まで動かす。ページに置いた後は本文左端）。縦範囲は持たず、
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
  ///
  /// # Errors
  ///
  /// 矩形の位置か幅が長さの値域に収まらないとき [`LengthOverflow`]。
  pub(in crate::typeset) fn place(
    &self,
    dx: Length,
    top: Length,
    height: Length,
  ) -> Result<Option<PlacedLink>, LengthOverflow> {
    if self.x1 <= self.x0 {
      return Ok(None);
    }
    return Ok(Some(PlacedLink {
      target: self.target.clone(),
      x: dx.try_add(self.x0)?,
      y: top,
      width: self.x1.try_sub(self.x0)?,
      height,
    }));
  }
}
