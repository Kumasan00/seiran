//! シェーピング済みグリフ列 [`GlyphRun`] と [`Glyph`]。

use std::ops::Range;

use crate::{color::Color, length::Length, project::FontFace};

/// 1 つの実体（フォント種別 × 候補）でシェーピングしたグリフ列
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphRun {
  /// テキストのフォントサイズ
  pub font_size: Length,
  /// 元のテキスト（シェーピング前）
  pub text: String,
  /// シェーピング結果のグリフ列
  pub glyphs: Vec<Glyph>,
  /// このグリフ列が使用する実体（[`crate::PublicationResources::font`] で描画資源を引く鍵）
  pub face: FontFace,
  /// テキスト色。`None` は既定色（黒）。
  /// `\color` かリンク色（style.toml の `[hyperref]`）が効いたテキストだけ `Some` になる。
  pub color: Option<Color>,
}

/// 単一グリフの配置情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
  /// グリフ ID
  pub gid: u32,
  /// グリフが属するクラスタの元テキスト範囲（[`GlyphRun::text`] に対するバイト位置。両端は文字境界）。
  /// 同じクラスタの複数グリフは同じ範囲を持つ — krilla はこの一致でクラスタを認識し `ActualText` にまとめる
  pub range: Range<usize>,
  /// x 方向の送り幅（フォントユニット）
  pub x_advance: i32,
  /// y 方向の送り幅（フォントユニット）
  pub y_advance: i32,
  /// x 方向のオフセット（フォントユニット）
  pub x_offset: i32,
  /// y 方向のオフセット（フォントユニット）
  pub y_offset: i32,
}
