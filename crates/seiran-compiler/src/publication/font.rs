//! フォントの描画契約 — krilla フォント構築設定 [`FontFaceConfig`] / [`VariationAxisConfig`] と
//! 基本メトリクス [`FontMetric`]。
//!
//! `PublicationFont` のフィールド型として描画バックエンドまで届く leaf 値型。
//! `crate::project::FontConfig` からの変換（`build_face_configs`）と OpenType テーブルからの
//! 取得（`build_font_metrics`）は `crate::typeset::font` が持ち、ここは値の形だけを所有する
//! （#535 で `typeset` から移設。renderer 側に同型の複製型を作らせない #305 / #372 の判断は維持）。

/// Krilla フォント構築に必要な設定（`crate::project::FontConfig` から renderer が要る値だけを取り出した最小表現）。
#[derive(Debug, Clone, PartialEq)]
pub struct FontFaceConfig {
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxisConfig>>,
}

/// バリアブルフォント軸の設定値（`crate::project::VariationAxis` の複製）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariationAxisConfig {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}

/// 1 フォントの基本メトリクス。
///
/// 値はフォントユニット系で、`descender` は OpenType の慣例どおり通常は負値。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetric {
  /// units-per-em（`head` テーブル由来）
  pub upem: f32,
  /// アセンダ（`hhea` テーブル由来、フォントユニット）
  pub ascender: f32,
  /// ディセンダ（`hhea` テーブル由来、フォントユニット、通常は負値）
  pub descender: f32,
}
