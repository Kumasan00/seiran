//! 行分割の出力（[`Line`] / [`PositionedBox`]）の定義

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
