//! シェーピング済みグリフ列 [`GlyphRun`] と [`Glyph`]。
//!
//! `PaintOp::DrawGlyphRun` がそのまま載せる描画契約の値型で、シェイピング処理そのものは
//! `crate::typeset::font` が持つ（#535 で値型だけをこちらへ移設）。フォントサイズ・色の
//! 単位変換は render（`seiran-pdf`）が行う。

use std::ops::Range;

use crate::{color::Color, length::Length, project::FontType};

/// 1 つのフォント種別でシェーピングしたグリフ列
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphRun {
  /// テキストのフォントサイズ
  pub font_size: Length,
  /// 元のテキスト（シェーピング前）
  pub text: String,
  /// シェーピング結果のグリフ列
  pub glyphs: Vec<Glyph>,
  /// このグリフ列が使用するフォント種別
  pub font_type: FontType,
  /// テキスト色。`None` は既定色（黒）を意味し、render は塗り色を設定しない。
  /// `\color[color=#rrggbb]{...}` 由来のテキストだけ `Some` になる。
  pub color: Option<Color>,
}

/// 単一グリフの配置情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
  /// グリフ ID
  pub gid: u32,
  /// グリフのテキスト範囲（元のテキストに対するバイトインデックスの範囲）
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
