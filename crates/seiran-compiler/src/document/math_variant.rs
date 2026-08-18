//! 数式中のフォントスタイル指定 [`MathVariant`]。

/// 数式中のフォントスタイル指定
///
/// `\mathbold{...}` 等のコマンドで指定され、ローワリング層で
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）の
/// 該当コードポイントへ ASCII 英字・数字・Greek 文字を変換する。
///
/// `FontKind::Math` のままで字形を切り替える設計のため、
/// 数式フォントが対応するグリフを持っている前提で動作する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathVariant {
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

impl MathVariant {
  /// コマンド名から対応する `MathVariant` を解決する
  ///
  /// 数式モード内で `evaluate_math_command` から呼び出される。
  /// 未対応の名前は `None` を返す。
  #[must_use]
  pub(crate) fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "mathserif" => Some(MathVariant::Serif),
      "mathitalic" => Some(MathVariant::Italic),
      "mathbold" => Some(MathVariant::Bold),
      "mathbolditalic" => Some(MathVariant::BoldItalic),
      "mathsans" => Some(MathVariant::Sans),
      "mathsansitalic" => Some(MathVariant::SansItalic),
      "mathsansbold" => Some(MathVariant::SansBold),
      "mathsansbolditalic" => Some(MathVariant::SansBoldItalic),
      "mathmono" => Some(MathVariant::Mono),
      "mathdoublestruck" => Some(MathVariant::DoubleStruck),
      "mathscript" => Some(MathVariant::Script),
      "mathcalligraphic" => Some(MathVariant::Calligraphic),
      "mathfraktur" => Some(MathVariant::Fraktur),
      "mathscriptbold" => Some(MathVariant::ScriptBold),
      "mathfrakturbold" => Some(MathVariant::FrakturBold),
      _ => None,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn math_variant_from_command_name_resolves_all_styles() {
    // Arrange & Act & Assert — 15 個のスタイルコマンドが正しく解決される
    assert_eq!(MathVariant::from_command_name("mathserif"), Some(MathVariant::Serif));
    assert_eq!(MathVariant::from_command_name("mathitalic"), Some(MathVariant::Italic));
    assert_eq!(MathVariant::from_command_name("mathbold"), Some(MathVariant::Bold));
    assert_eq!(MathVariant::from_command_name("mathbolditalic"), Some(MathVariant::BoldItalic));
    assert_eq!(MathVariant::from_command_name("mathsans"), Some(MathVariant::Sans));
    assert_eq!(MathVariant::from_command_name("mathsansitalic"), Some(MathVariant::SansItalic));
    assert_eq!(MathVariant::from_command_name("mathsansbold"), Some(MathVariant::SansBold));
    assert_eq!(MathVariant::from_command_name("mathsansbolditalic"), Some(MathVariant::SansBoldItalic));
    assert_eq!(MathVariant::from_command_name("mathmono"), Some(MathVariant::Mono));
    assert_eq!(MathVariant::from_command_name("mathdoublestruck"), Some(MathVariant::DoubleStruck));
    assert_eq!(MathVariant::from_command_name("mathscript"), Some(MathVariant::Script));
    assert_eq!(MathVariant::from_command_name("mathcalligraphic"), Some(MathVariant::Calligraphic));
    assert_eq!(MathVariant::from_command_name("mathfraktur"), Some(MathVariant::Fraktur));
    assert_eq!(MathVariant::from_command_name("mathscriptbold"), Some(MathVariant::ScriptBold));
    assert_eq!(MathVariant::from_command_name("mathfrakturbold"), Some(MathVariant::FrakturBold));
  }

  #[test]
  fn math_variant_from_command_name_rejects_unknown() {
    // Arrange & Act & Assert — 未知名は None
    assert_eq!(MathVariant::from_command_name("mathrm"), None);
    assert_eq!(MathVariant::from_command_name("mathbf"), None);
    assert_eq!(MathVariant::from_command_name("foo"), None);
    assert_eq!(MathVariant::from_command_name(""), None);
  }
}
