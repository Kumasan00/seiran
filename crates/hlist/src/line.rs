//! 行分割の出力（[`Line`] / [`PositionedBox`] / [`LineLink`]）の定義

use types::LinkTarget;

use crate::hitem::HBoxContent;

/// 行分割で確定した 1 行
///
/// `height` / `depth` は行内ボックスの `dy ± height/depth` の最大値。
/// `is_last` は段落最終行または強制改行（`\\`）による行で、両端揃え時に
/// 伸縮を適用しない（ragged のまま残す）ためのフラグ。
#[derive(Debug, Clone)]
pub struct Line {
  /// 行内の配置済みボックス（左から順）
  pub boxes: Vec<PositionedBox>,
  /// ベースラインから上の高さ（pt）
  pub height: f32,
  /// ベースラインから下の深さ（pt、正値）
  pub depth: f32,
  /// 段落最終行・強制改行による行か
  pub is_last: bool,
  /// この行に含まれるクリック可能なリンク領域（機構 B・行頭からの水平範囲）
  ///
  /// 1 つのリンクが折り返しをまたぐ場合は行ごとに 1 つの矩形へ分割される。
  pub links: Vec<LineLink>,
}

/// 行内のリンク領域（クリック矩形の水平範囲）
///
/// `x0` / `x1` は行頭（本文左端）からの水平オフセット（pt）。縦範囲は所属する
/// [`Line`] の `height` / `depth` から `break_pages` が確定する。
#[derive(Debug, Clone)]
pub struct LineLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 領域左端の行頭からの水平オフセット（pt）
  pub x0: f32,
  /// 領域右端の行頭からの水平オフセット（pt）
  pub x1: f32,
}

/// 行内に配置されたボックス
///
/// `x` は行頭（本文左端）からの水平オフセット、`dy` はベースラインからの
/// 縦オフセット（正で上方向）。
#[derive(Debug, Clone)]
pub struct PositionedBox {
  /// ボックスの内容
  pub content: HBoxContent,
  /// 行頭からの水平オフセット（pt）
  pub x: f32,
  /// ベースラインからの縦オフセット（pt、正で上方向）
  pub dy: f32,
  /// 幅（pt）
  pub width: f32,
}
