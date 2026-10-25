//! 最終的なフォント種別 [`FontType`] と、その実体（種別 × 候補）の鍵 [`FontFace`]。

use std::fmt;

use strum::{IntoStaticStr, VariantArray};

/// 言語とスタイルが確定した 19 フォント種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum FontType {
  /// Serif 標準フォント
  Serif,
  /// Serif 太字フォント
  SerifBold,
  /// Serif イタリックフォント
  SerifItalic,
  /// Serif 太字イタリックフォント
  SerifBoldItalic,
  /// Sans Serif 標準フォント
  SansSerif,
  /// Sans Serif 太字フォント
  SansSerifBold,
  /// Sans Serif イタリックフォント
  SansSerifItalic,
  /// Sans Serif 太字イタリックフォント
  SansSerifBoldItalic,
  /// Monospace 標準フォント
  Monospace,
  /// Monospace 太字フォント
  MonospaceBold,
  /// Monospace イタリックフォント
  MonospaceItalic,
  /// Monospace 太字イタリックフォント
  MonospaceBoldItalic,
  /// 数式用フォント（OpenType Math テーブル対応）
  Math,
  /// 日本語用 Serif 標準フォント
  JapaneseSerif,
  /// 日本語用 Serif 太字フォント
  JapaneseSerifBold,
  /// 日本語用 Sans Serif 標準フォント
  JapaneseSansSerif,
  /// 日本語用 Sans Serif 太字フォント
  JapaneseSansSerifBold,
  /// 日本語用 Monospace 標準フォント
  JapaneseMonospace,
  /// 日本語用 Monospace 太字フォント
  JapaneseMonospaceBold,
}

impl FontType {
  /// TOML でこのフォント種別を指す `snake_case` のキーを返す
  ///
  /// `[font_configs.<key>]`（本文用は `[[font_configs.<key>]]`）のキーと一致する。`Debug` は `PascalCase` で
  /// TOML キーと一致しないので、キーが要る箇所はこちらを使う（設定パスの表記は [`FontFace`] の `Display`）。
  #[must_use]
  pub fn as_toml_key(self) -> &'static str { return self.into(); }
}

/// 実体（枠 [`FontType`] × 候補の位置）の鍵
///
/// フィールドは非公開で、crate の外では [`crate::PublicationResources::faces`] と [`crate::GlyphRun::face`]
/// から受け取るだけ — 資源に無い実体を指す値を crate の外で作れない。`math` の候補は常に 1 つ（位置 0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFace {
  /// 枠
  font_type: FontType,
  /// 枠の中の候補の位置（config.toml の配列の添字。先頭が 0）
  candidate: usize,
}

impl FontFace {
  /// 数式フォントの実体（`math` は候補を 1 つだけ持つ）
  pub(crate) const MATH: Self = Self::primary(FontType::Math);

  /// 枠 `font_type` の `candidate` 番目の候補を指す鍵を作る。
  pub(crate) const fn new(font_type: FontType, candidate: usize) -> Self {
    return Self {
      font_type,
      candidate,
    };
  }

  /// 枠 `font_type` の先頭の候補を指す鍵を作る。
  pub(crate) const fn primary(font_type: FontType) -> Self { return Self::new(font_type, 0); }

  /// この実体の枠を返す。
  #[must_use]
  pub fn font_type(self) -> FontType { return self.font_type; }

  /// この実体の枠の中の候補の位置を返す（先頭が 0）。
  #[must_use]
  pub fn candidate(self) -> usize { return self.candidate; }
}

/// 設定パス・診断の枠表記: `math` はキーだけ、本文の枠は `<キー>[<候補の位置>]`（例 `japanese_serif[1]`）。
impl fmt::Display for FontFace {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    let key = self.font_type.as_toml_key();
    return match self.font_type {
      FontType::Math => formatter.write_str(key),
      FontType::Serif
      | FontType::SerifBold
      | FontType::SerifItalic
      | FontType::SerifBoldItalic
      | FontType::SansSerif
      | FontType::SansSerifBold
      | FontType::SansSerifItalic
      | FontType::SansSerifBoldItalic
      | FontType::Monospace
      | FontType::MonospaceBold
      | FontType::MonospaceItalic
      | FontType::MonospaceBoldItalic
      | FontType::JapaneseSerif
      | FontType::JapaneseSerifBold
      | FontType::JapaneseSansSerif
      | FontType::JapaneseSansSerifBold
      | FontType::JapaneseMonospace
      | FontType::JapaneseMonospaceBold => write!(formatter, "{key}[{}]", self.candidate),
    };
  }
}

#[cfg(test)]
mod tests {
  use super::{FontFace, FontType};

  #[test]
  fn face_display_uses_index_for_body_slots_and_bare_key_for_math() {
    assert_eq!(FontFace::new(FontType::JapaneseSerif, 1).to_string(), "japanese_serif[1]");
    assert_eq!(FontFace::primary(FontType::Serif).to_string(), "serif[0]");
    assert_eq!(FontFace::MATH.to_string(), "math");
  }
}
