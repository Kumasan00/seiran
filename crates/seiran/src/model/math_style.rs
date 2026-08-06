//! 数式中のフォントスタイル指定 [`MathStyle`]。

/// 数式中のフォントスタイル指定
///
/// `\mathbold{...}` 等のコマンドで指定され、ローワリング層で
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）の
/// 該当コードポイントへ ASCII 英字・数字・Greek 文字を変換する。
///
/// `FontKind::Math` のままで字形を切り替える設計のため、
/// 数式フォントが対応するグリフを持っている前提で動作する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
  /// `\mathserif` — セリフ立体（素通し）
  Serif,
  /// `\mathitalic` — セリフイタリック
  Italic,
  /// `\mathbold` — セリフ太字
  Bold,
  /// `\mathbolditalic` — セリフ太字イタリック
  BoldItalic,
  /// `\mathsans` — サンセリフ立体
  Sans,
  /// `\mathsansitalic` — サンセリフイタリック
  SansItalic,
  /// `\mathsansbold` — サンセリフ太字
  SansBold,
  /// `\mathsansbolditalic` — サンセリフ太字イタリック
  SansBoldItalic,
  /// `\mathmono` — 等幅
  Mono,
  /// `\mathdoublestruck` — 黒板太字（double-struck, ℝ ℂ ℕ 等）
  DoubleStruck,
  /// `\mathscript` — スクリプト（roundhand, 花文字）
  Script,
  /// `\mathcalligraphic` — カリグラフィー（chancery, 花文字の筆記体）
  ///
  /// スクリプトと同一の基底コードポイントに異体字セレクタ VS1（U+FE00）を付与して
  /// chancery 字形を要求する。Unicode の数式異体字シーケンスに対応した数式フォントでのみ
  /// chancery 字形が選ばれ、非対応フォントでは VS1 が無視されてスクリプト字形に
  /// フォールバックする（フォント非依存対応は OpenType `ss01` を使う別 issue で行う）。
  Calligraphic,
  /// `\mathfraktur` — フラクトゥール（ドイツ文字, ℌ ℑ ℜ 等）
  Fraktur,
  /// `\mathscriptbold` — 太字スクリプト（bold roundhand, 花文字の太字）
  ScriptBold,
  /// `\mathfrakturbold` — 太字フラクトゥール（bold ドイツ文字）
  FrakturBold,
}

impl MathStyle {
  /// コマンド名から対応する `MathStyle` を解決する
  ///
  /// 数式モード内で `evaluate_math_command` から呼び出される。
  /// 未対応の名前は `None` を返す。
  #[must_use]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "mathserif" => Some(MathStyle::Serif),
      "mathitalic" => Some(MathStyle::Italic),
      "mathbold" => Some(MathStyle::Bold),
      "mathbolditalic" => Some(MathStyle::BoldItalic),
      "mathsans" => Some(MathStyle::Sans),
      "mathsansitalic" => Some(MathStyle::SansItalic),
      "mathsansbold" => Some(MathStyle::SansBold),
      "mathsansbolditalic" => Some(MathStyle::SansBoldItalic),
      "mathmono" => Some(MathStyle::Mono),
      "mathdoublestruck" => Some(MathStyle::DoubleStruck),
      "mathscript" => Some(MathStyle::Script),
      "mathcalligraphic" => Some(MathStyle::Calligraphic),
      "mathfraktur" => Some(MathStyle::Fraktur),
      "mathscriptbold" => Some(MathStyle::ScriptBold),
      "mathfrakturbold" => Some(MathStyle::FrakturBold),
      _ => None,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn math_style_from_command_name_resolves_all_styles() {
    // Arrange & Act & Assert — 15 個のスタイルコマンドが正しく解決される
    assert_eq!(MathStyle::from_command_name("mathserif"), Some(MathStyle::Serif));
    assert_eq!(MathStyle::from_command_name("mathitalic"), Some(MathStyle::Italic));
    assert_eq!(MathStyle::from_command_name("mathbold"), Some(MathStyle::Bold));
    assert_eq!(MathStyle::from_command_name("mathbolditalic"), Some(MathStyle::BoldItalic));
    assert_eq!(MathStyle::from_command_name("mathsans"), Some(MathStyle::Sans));
    assert_eq!(MathStyle::from_command_name("mathsansitalic"), Some(MathStyle::SansItalic));
    assert_eq!(MathStyle::from_command_name("mathsansbold"), Some(MathStyle::SansBold));
    assert_eq!(MathStyle::from_command_name("mathsansbolditalic"), Some(MathStyle::SansBoldItalic));
    assert_eq!(MathStyle::from_command_name("mathmono"), Some(MathStyle::Mono));
    assert_eq!(MathStyle::from_command_name("mathdoublestruck"), Some(MathStyle::DoubleStruck));
    assert_eq!(MathStyle::from_command_name("mathscript"), Some(MathStyle::Script));
    assert_eq!(MathStyle::from_command_name("mathcalligraphic"), Some(MathStyle::Calligraphic));
    assert_eq!(MathStyle::from_command_name("mathfraktur"), Some(MathStyle::Fraktur));
    assert_eq!(MathStyle::from_command_name("mathscriptbold"), Some(MathStyle::ScriptBold));
    assert_eq!(MathStyle::from_command_name("mathfrakturbold"), Some(MathStyle::FrakturBold));
  }

  #[test]
  fn math_style_from_command_name_rejects_unknown() {
    // Arrange & Act & Assert — 未知名は None
    assert_eq!(MathStyle::from_command_name("mathrm"), None);
    assert_eq!(MathStyle::from_command_name("mathbf"), None);
    assert_eq!(MathStyle::from_command_name("foo"), None);
    assert_eq!(MathStyle::from_command_name(""), None);
  }
}
