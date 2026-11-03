//! フォントの描画契約 — krilla フォント構築設定 [`FontFaceConfig`] と基本メトリクス [`FontMetrics`]。

use std::sync::Arc;

use crate::project::VariationAxis;

/// フォントバイト列を複製せず `Arc<dyn AsRef<[u8]> + Send + Sync>`（krilla の `Data` と fontdb の
/// `Source::Binary` が受け取る形）へ渡すための包み。
///
/// `Arc<[u8]>` はスライスが `Sized` でないため、そのままでは `dyn AsRef<[u8]>` へ unsize できない。
pub(super) struct FontBytes(pub(super) Arc<[u8]>);

impl AsRef<[u8]> for FontBytes {
  /// 包んだ共有ハンドルの中身を借用する。
  fn as_ref(&self) -> &[u8] { return &self.0; }
}

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
