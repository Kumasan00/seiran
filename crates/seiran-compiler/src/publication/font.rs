//! フォントの描画契約 — krilla フォント構築設定 [`FontFaceConfig`] と基本メトリクス [`FontMetrics`]。

use crate::project::VariationAxis;

/// Krilla フォント構築に必要な設定（`crate::project::FontConfig` から取り出した値）。
#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceConfig {
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxis>>,
}

/// 1 フォントの基本メトリクス。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
  /// units-per-em（`head` テーブル由来）
  pub upem: f32,
  /// アセンダ（`hhea` テーブル由来、フォントユニット）
  pub ascender: f32,
  /// ディセンダ（`hhea` テーブル由来、フォントユニット、通常は負値）
  pub descender: f32,
}
