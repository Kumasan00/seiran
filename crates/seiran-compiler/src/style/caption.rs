//! 図・表のキャプションスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::{
  document::Typeface,
  length::{Length, positive},
  style::NumberTitleTemplate,
};

/// キャプションの共通設定。
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct CaptionStyle {
  /// キャプションの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(dive)]
  pub format: NumberTitleTemplate,
  /// キャプションのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// キャプション全体（番号部分と本文部分）の書体
  pub typeface: Typeface,
}

impl Default for CaptionStyle {
  fn default() -> Self {
    return Self {
      format: NumberTitleTemplate::parse("{number}: {title}"),
      font_size: Length::pt(11.0),
      typeface: Typeface::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::CaptionStyle;
  use crate::document::Typeface;

  #[test]
  fn deserializes_partial_table_with_default_font_size() {
    let toml = "format = \"Figure {number}: {title}\"\n";
    let style: CaptionStyle = toml::from_str(toml).unwrap();
    assert_eq!(style.format.as_str(), "Figure {number}: {title}");
    assert!((style.font_size.to_pt() - 11.0).abs() < f32::EPSILON);
    assert_eq!(style.typeface, Typeface::Serif);
  }

  #[test]
  fn deserialize_overrides_typeface() {
    let toml = "typeface = \"sans_serif\"\n";
    let style: CaptionStyle = toml::from_str(toml).expect("`[figure.caption]` の本体として読めるはず");
    assert_eq!(style.typeface, Typeface::SansSerif);
  }
}
