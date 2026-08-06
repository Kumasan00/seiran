//! `model` / `font` クレートに依存しない、render 境界専用の自己完結した leaf 型。
//!
//! `Publication` はここに定義する型だけで完結し、compiler 側（`seiran`）の内部型を
//! 一切参照しない。座標は pt 単位の `f32` で表現する（PDF 出力自体が f32 精度のため、
//! ここで精度を落としても実際の描画結果は変わらない）。

use std::ops::Range;

/// 19 フォント種別（`seiran` crate の `font::FontType` の複製）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontType {
  /// Serif 標準
  Serif,
  /// Serif 太字
  SerifBold,
  /// Serif イタリック
  SerifItalic,
  /// Serif 太字イタリック
  SerifBoldItalic,
  /// Sans Serif 標準
  SansSerif,
  /// Sans Serif 太字
  SansSerifBold,
  /// Sans Serif イタリック
  SansSerifItalic,
  /// Sans Serif 太字イタリック
  SansSerifBoldItalic,
  /// Monospace 標準
  Monospace,
  /// Monospace 太字
  MonospaceBold,
  /// Monospace イタリック
  MonospaceItalic,
  /// Monospace 太字イタリック
  MonospaceBoldItalic,
  /// 数式用
  Math,
  /// 日本語 Serif 標準
  JapaneseSerif,
  /// 日本語 Serif 太字
  JapaneseSerifBold,
  /// 日本語 Sans Serif 標準
  JapaneseSansSerif,
  /// 日本語 Sans Serif 太字
  JapaneseSansSerifBold,
  /// 日本語 Monospace 標準
  JapaneseMonospace,
  /// 日本語 Monospace 太字
  JapaneseMonospaceBold,
}

impl FontType {
  /// 全フォント種別を宣言順に並べた配列（`seiran` crate の `font::FontType::ALL` と同じ並び）。
  ///
  /// `HashMap` はイテレーション順を保証しないため、`ResourceBundle::new` はこの配列の順序で
  /// フォントを構築する（フォント検証エラーの選択順序を決定的にし、`FontFaceInput` が全種別分
  /// 揃っているかの完全性チェックにも使う）。
  pub const ALL: [FontType; 19] = [
    FontType::Serif,
    FontType::SerifBold,
    FontType::SerifItalic,
    FontType::SerifBoldItalic,
    FontType::SansSerif,
    FontType::SansSerifBold,
    FontType::SansSerifItalic,
    FontType::SansSerifBoldItalic,
    FontType::Monospace,
    FontType::MonospaceBold,
    FontType::MonospaceItalic,
    FontType::MonospaceBoldItalic,
    FontType::Math,
    FontType::JapaneseSerif,
    FontType::JapaneseSerifBold,
    FontType::JapaneseSansSerif,
    FontType::JapaneseSansSerifBold,
    FontType::JapaneseMonospace,
    FontType::JapaneseMonospaceBold,
  ];
}

/// フォント構築に必要な設定（`font::FontFaceConfig` の複製 + フォントバイト本体）。
#[derive(Debug, Clone)]
pub struct FontFaceInput {
  /// フォントファイルの生バイト列
  pub bytes: Vec<u8>,
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxisInput>>,
}

/// バリアブルフォント軸の設定値（`font::VariationAxisConfig` の複製）。
#[derive(Debug, Clone, Copy)]
pub struct VariationAxisInput {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}

/// 基本フォントメトリクス（`font::FontMetric` の複製）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetric {
  /// units-per-em
  pub upem: f32,
  /// アセンダ（フォントユニット）
  pub ascender: f32,
  /// ディセンダ（フォントユニット、通常は負値）
  pub descender: f32,
}

/// 1 つのフォント種別でシェーピングしたグリフ列（`font::GlyphRun` の複製、`font_size`/`color` は
/// pt 単位の `f32` / `[u8;3]` に単純化済み）。
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
  /// フォントサイズ（pt）
  pub font_size: f32,
  /// 元のテキスト（シェーピング前）
  pub text: String,
  /// シェーピング結果のグリフ列
  pub glyphs: Vec<Glyph>,
  /// このグリフ列が使用するフォント種別
  pub font_type: FontType,
  /// テキスト色（`None` は既定色＝黒）
  pub color: Option<[u8; 3]>,
}

/// 単一グリフの配置情報（`font::Glyph` と同一構造）。
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
  /// グリフ ID
  pub gid: u32,
  /// グリフのテキスト範囲
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
