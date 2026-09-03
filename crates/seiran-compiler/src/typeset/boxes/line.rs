//! 行分割の出力。

use crate::{
  length::Length,
  typeset::boxes::{
    hitem::{HBoxContent, HItem},
    link::LinkTarget,
  },
};

/// 行分割で確定した 1 行
///
/// `height` / `depth` は行内ボックスの `dy ± height/depth` の最大値。
/// `is_last` は段落最終行または強制改行（`\\`）による行で、両端揃え時に
/// 伸縮を適用しない（ragged のまま残す）ためのフラグ。
#[derive(Debug, Clone)]
pub(crate) struct Line {
  /// 行内の配置済みボックス（左から順）
  pub boxes: Vec<PositionedBox>,
  /// ベースラインから上の高さ
  pub height: Length,
  /// ベースラインから下の深さ（正値）
  pub depth: Length,
  /// 段落最終行・強制改行による行か
  #[cfg_attr(
    not(test),
    expect(
      dead_code,
      reason = "書き込むだけで組版は読まない（crate 内の `#[cfg(test)]` が行分割の結果を検証する）"
    )
  )]
  pub is_last: bool,
  /// この行に含まれるクリック可能なリンク領域（機構 B・行頭からの水平範囲）
  ///
  /// 1 つのリンクが折り返しをまたぐ場合は行ごとに 1 つの矩形へ分割される。
  pub links: Vec<LineLink>,
  /// この行に含まれる脚注（出現順）
  ///
  /// `typeset::breaking::break_pages` がこの行を配置する際に本体を行分割し、
  /// 実効ページ下限（`page_limit` から脚注ぶんを差し引いた値）へ織り込む。
  pub footnotes: Vec<LineFootnote>,
  /// この行に含まれる索引語（`\index{語}`、出現順）
  ///
  /// `typeset::breaking::break_pages` がこの行の所属ページを索引語の出現ページとして扱い、
  /// 重複除去のうえ `Page::index_entries` へ集約する。
  pub index_marks: Vec<LineIndexEntry>,
}

impl Line {
  /// 行の実効幅（最も右へ伸びるボックスの右端）
  ///
  /// `boxes` は左から順に積まれるが、両端揃えの伸縮や約物の負アキで最後の箱が最右とは限らないため、
  /// 右端の最大値を取る。
  #[must_use]
  pub(crate) fn width(&self) -> Length {
    return self.boxes.iter().map(|placed| return placed.x + placed.width).fold(Length::ZERO, Length::max);
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
    for positioned in &mut self.boxes {
      positioned.x += dx;
    }
    for link in &mut self.links {
      link.x0 += dx;
      link.x1 += dx;
    }
  }
}

/// 行内の脚注（`\footnote{...}`）本体
#[derive(Debug, Clone)]
pub(crate) struct LineFootnote {
  /// 発番済みの表示番号（[`HItem::Footnote`] から素通し）
  pub number: u32,
  /// 出現順の識別子（0 起点。[`HItem::Footnote`] から素通し）
  pub index: u32,
  /// 脚注本体（計測済みの水平アイテム列。ページ下部配置時に行分割する）
  pub items: Vec<HItem>,
  /// 脚注本体の行送り（支配的フォントサイズ × 行高係数）
  pub leading: Length,
}

/// 行内の索引語（`\index{語}`）1 件
#[derive(Debug, Clone)]
pub(crate) struct LineIndexEntry {
  /// 索引語
  pub word: String,
  /// 読みソートキー（`[reading=...]`）
  pub reading: Option<String>,
}

/// 行内のリンク領域（クリック矩形の水平範囲）
///
/// `x0` / `x1` は行頭（本文左端）からの水平オフセット（pt）。縦範囲は所属する
/// [`Line`] の `height` / `depth` から `break_pages` が確定する。
#[derive(Debug, Clone)]
pub(crate) struct LineLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 領域左端の行頭からの水平オフセット
  pub x0: Length,
  /// 領域右端の行頭からの水平オフセット
  pub x1: Length,
}

/// 行内に配置されたボックス
///
/// `x` は行頭（本文左端）からの水平オフセット、`dy` はベースラインからの
/// 縦オフセット（正で上方向）。
#[derive(Debug, Clone)]
pub(crate) struct PositionedBox {
  /// ボックスの内容
  pub content: HBoxContent,
  /// 行頭からの水平オフセット
  pub x: Length,
  /// ベースラインからの縦オフセット（正で上方向）
  pub dy: Length,
  /// 幅
  pub width: Length,
}
