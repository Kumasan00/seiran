//! 最終的なフォント種別 [`FontType`]。

/// 言語とスタイルが確定した 19 フォント種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontType {
  /// Serif 標準フォント（通常の太さ、通常のゆがみ）
  Serif,
  /// Serif 太字フォント（太字の太さ、通常のゆがみ）
  SerifBold,
  /// Serif イタリックフォント（通常の太さ、右に傾いた形）
  SerifItalic,
  /// Serif 太字イタリックフォント（太字の太さ、右に傾いた形）
  SerifBoldItalic,
  /// Sans Serif 標準フォント（通常の太さ、通常のゆがみ）
  SansSerif,
  /// Sans Serif 太字フォント（太字の太さ、通常のゆがみ）
  SansSerifBold,
  /// Sans Serif イタリックフォント（通常の太さ、右に傾いた形）
  SansSerifItalic,
  /// Sans Serif 太字イタリックフォント（太字の太さ、右に傾いた形）
  SansSerifBoldItalic,
  /// Monospace 標準フォント（等幅、通常の太さ）
  Monospace,
  /// Monospace 太字フォント（等幅、太字の太さ）
  MonospaceBold,
  /// Monospace イタリックフォント（等幅、右に傾いた形）
  MonospaceItalic,
  /// Monospace 太字イタリックフォント（等幅、太字で傾いた形）
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

impl std::fmt::Display for FontType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let name = match self {
      FontType::Serif => "Serif",
      FontType::SerifBold => "Serif Bold",
      FontType::SerifItalic => "Serif Italic",
      FontType::SerifBoldItalic => "Serif Bold Italic",
      FontType::SansSerif => "Sans Serif",
      FontType::SansSerifBold => "Sans Serif Bold",
      FontType::SansSerifItalic => "Sans Serif Italic",
      FontType::SansSerifBoldItalic => "Sans Serif Bold Italic",
      FontType::Monospace => "Monospace",
      FontType::MonospaceBold => "Monospace Bold",
      FontType::MonospaceItalic => "Monospace Italic",
      FontType::MonospaceBoldItalic => "Monospace Bold Italic",
      FontType::Math => "Math",
      FontType::JapaneseSerif => "Japanese Serif",
      FontType::JapaneseSerifBold => "Japanese Serif Bold",
      FontType::JapaneseSansSerif => "Japanese Sans Serif",
      FontType::JapaneseSansSerifBold => "Japanese Sans Serif Bold",
      FontType::JapaneseMonospace => "Japanese Monospace",
      FontType::JapaneseMonospaceBold => "Japanese Monospace Bold",
    };
    return write!(f, "{name}");
  }
}

impl FontType {
  /// 全フォント種別を宣言順に並べた配列
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

  /// TOML でこのフォント種別を指す `snake_case` のキーを返す
  ///
  /// `[font_configs.<key>]` セクションのキーと一致し、診断メッセージで設定パスを
  /// 表示する際の正規表記としても使用されます（`Debug` フォーマットは `PascalCase` で
  /// ユーザの書いた TOML キーと一致しないため、エラーパスにはこちらを使ってください）。
  #[must_use]
  pub fn as_toml_key(self) -> &'static str {
    return match self {
      FontType::Serif => "serif",
      FontType::SerifBold => "serif_bold",
      FontType::SerifItalic => "serif_italic",
      FontType::SerifBoldItalic => "serif_bold_italic",
      FontType::SansSerif => "sans_serif",
      FontType::SansSerifBold => "sans_serif_bold",
      FontType::SansSerifItalic => "sans_serif_italic",
      FontType::SansSerifBoldItalic => "sans_serif_bold_italic",
      FontType::Monospace => "monospace",
      FontType::MonospaceBold => "monospace_bold",
      FontType::MonospaceItalic => "monospace_italic",
      FontType::MonospaceBoldItalic => "monospace_bold_italic",
      FontType::Math => "math",
      FontType::JapaneseSerif => "japanese_serif",
      FontType::JapaneseSerifBold => "japanese_serif_bold",
      FontType::JapaneseSansSerif => "japanese_sans_serif",
      FontType::JapaneseSansSerifBold => "japanese_sans_serif_bold",
      FontType::JapaneseMonospace => "japanese_monospace",
      FontType::JapaneseMonospaceBold => "japanese_monospace_bold",
    };
  }
}
