//! 図・表のキャプションスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Deserializer};

use crate::{
  document::Typeface,
  length::{Length, positive},
  style::NumberTitleTemplate,
};

/// キャプションの共通設定。
///
/// TOML のスキーマは [`CaptionStyleOverride`]（書いたフィールドだけを種類ごとの既定へ重ねる。[`deserialize_over`]）。
#[derive(Debug, Clone, Validate)]
#[garde(allow_unvalidated)]
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

/// [`CaptionStyle`] の各フィールドを `Option<_>` で覆った差分指定型（`[figure.caption]` / `[table.caption]` /
/// `[figure.subfigure.caption]` の TOML スキーマ）。`None` のフィールドは種類ごとの既定のまま残す
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct CaptionStyleOverride {
  /// 書式テンプレート
  format: Option<NumberTitleTemplate>,
  /// フォントサイズ
  font_size: Option<Length>,
  /// 書体
  typeface: Option<Typeface>,
}

/// キャプションの表を読み、書いたフィールドだけで種類ごとの既定 `base` を上書きする
///
/// 図・表・副図の `caption` フィールドの `deserialize_with` が、それぞれの既定を `base` に渡して呼ぶ。
///
/// # Errors
///
/// 未知のキー・値の型の誤りは `deserializer` のエラーとして返す。
pub(super) fn deserialize_over<'de, D: Deserializer<'de>>(
  deserializer: D,
  base: CaptionStyle,
) -> Result<CaptionStyle, D::Error> {
  let CaptionStyleOverride {
    format,
    font_size,
    typeface,
  } = CaptionStyleOverride::deserialize(deserializer)?;
  return Ok(CaptionStyle {
    format: format.unwrap_or(base.format),
    font_size: font_size.unwrap_or(base.font_size),
    typeface: typeface.unwrap_or(base.typeface),
  });
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Deserializer};

  use super::{CaptionStyle, deserialize_over};
  use crate::{document::Typeface, length::Length, style::NumberTitleTemplate};

  /// キャプションの表を `caption` に持つテスト用の外側の表（既定は `"Base {title}"`・9pt・sans-serif）
  #[derive(Debug, Deserialize)]
  struct Holder {
    /// 既定に重ねて読むキャプション
    #[serde(deserialize_with = "over_base")]
    caption: CaptionStyle,
  }

  /// テスト用の既定へ重ねて読む
  fn over_base<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CaptionStyle, D::Error> {
    let base = CaptionStyle {
      format: NumberTitleTemplate::parse("Base {title}"),
      font_size: Length::pt(9.0),
      typeface: Typeface::SansSerif,
    };
    return deserialize_over(deserializer, base);
  }

  #[test]
  fn unwritten_fields_keep_the_given_default() {
    let holder: Holder = toml::from_str("[caption]\nfont_size = \"12pt\"\n").unwrap();

    assert_eq!(holder.caption.format.as_str(), "Base {title}");
    assert_eq!(holder.caption.font_size, Length::pt(12.0));
    assert_eq!(holder.caption.typeface, Typeface::SansSerif);
  }

  #[test]
  fn written_fields_override_the_given_default() {
    let holder: Holder =
      toml::from_str("[caption]\nformat = \"Figure {number}: {title}\"\ntypeface = \"serif\"\n").unwrap();

    assert_eq!(holder.caption.format.as_str(), "Figure {number}: {title}");
    assert_eq!(holder.caption.typeface, Typeface::Serif);
  }

  #[test]
  fn unknown_key_is_rejected() {
    assert!(toml::from_str::<Holder>("[caption]\nsize = \"12pt\"\n").is_err());
  }
}
