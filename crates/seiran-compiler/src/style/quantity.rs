//! 数値・単位・量（`\num` 等）のスタイル設定型（`[quantity]` テーブル）。

use garde::Validate;
use serde::Deserialize;

use crate::document::Typeface;

/// 数値の書式と書体（`[quantity]` テーブル）
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct QuantityStyle {
  /// 数値の書体（本文・数式とも。外側の書体を継がない）。`math` は数式フォントの立体。イタリック系は拒否する
  #[garde(custom(upright_typeface))]
  pub typeface: Typeface,
  /// 小数点の記号
  pub decimal_marker: DecimalMarker,
  /// 3 桁区切り（整数部は右から・小数部は左から）。小数点と同じ記号は拒否する
  #[garde(custom(separator_differs_from_marker(self.decimal_marker)))]
  pub digit_group_separator: DigitGroupSeparator,
  /// 4 桁の部分（`1234`）も区切るか。整数部と小数部に別々に効く。区切りが `none` なら指定できない
  #[garde(custom(separator_exists_for_four_digits(self.digit_group_separator)))]
  pub group_four_digits: bool,
  /// 指数表記 `仮数 × 10^指数` の積の記号（前後のアキは medium 固定）
  pub exponent_product: ExponentProduct,
}

impl Default for QuantityStyle {
  fn default() -> Self {
    return Self {
      typeface: Typeface::Math,
      decimal_marker: DecimalMarker::Period,
      digit_group_separator: DigitGroupSeparator::Thin,
      group_four_digits: false,
      exponent_product: ExponentProduct::Times,
    };
  }
}

/// 小数点の記号
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecimalMarker {
  /// `.`
  Period,
  /// `,`
  Comma,
}

/// 3 桁区切り
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DigitGroupSeparator {
  /// 区切らない
  None,
  /// 細アキ（3mu）
  Thin,
  /// `,`
  Comma,
  /// `.`
  Period,
}

/// 指数表記の積の記号
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExponentProduct {
  /// U+00D7 MULTIPLICATION SIGN
  Times,
  /// U+22C5 DOT OPERATOR
  Dot,
}

/// `garde` 用バリデータ: 量の数値と単位を立体で組む（ISO 80000）ので、イタリック系の書体を拒否する。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn upright_typeface(value: &Typeface, _ctx: &()) -> garde::Result {
  return match value {
    Typeface::SerifItalic
    | Typeface::SerifBoldItalic
    | Typeface::SansSerifItalic
    | Typeface::SansSerifBoldItalic
    | Typeface::MonospaceItalic
    | Typeface::MonospaceBoldItalic => {
      Err(garde::Error::new("イタリック系の書体は指定できません（数値は立体で組みます）"))
    },
    Typeface::Serif
    | Typeface::SerifBold
    | Typeface::SansSerif
    | Typeface::SansSerifBold
    | Typeface::Monospace
    | Typeface::MonospaceBold
    | Typeface::Math => Ok(()),
  };
}

/// `garde` 用バリデータを返す: 3 桁区切りが小数点 `marker` と同じ記号でないことを要求する。
fn separator_differs_from_marker(marker: DecimalMarker) -> impl FnOnce(&DigitGroupSeparator, &()) -> garde::Result {
  return move |separator, _ctx| {
    let same = matches!(
      (marker, separator),
      (DecimalMarker::Period, DigitGroupSeparator::Period) | (DecimalMarker::Comma, DigitGroupSeparator::Comma)
    );
    if same {
      return Err(garde::Error::new("小数点（decimal_marker）と同じ記号は指定できません"));
    }
    return Ok(());
  };
}

/// `garde` 用バリデータを返す: 4 桁の部分を区切るなら、3 桁区切り `separator` が `none` でないことを要求する。
fn separator_exists_for_four_digits(separator: DigitGroupSeparator) -> impl FnOnce(&bool, &()) -> garde::Result {
  return move |group_four_digits, _ctx| {
    if *group_four_digits && separator == DigitGroupSeparator::None {
      return Err(garde::Error::new("digit_group_separator = \"none\" のときは true を指定できません"));
    }
    return Ok(());
  };
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{DecimalMarker, DigitGroupSeparator, ExponentProduct, QuantityStyle};
  use crate::document::Typeface;

  #[test]
  fn defaults_match_the_epic_table() {
    let style = QuantityStyle::default();

    assert_eq!(style.typeface, Typeface::Math);
    assert_eq!(style.decimal_marker, DecimalMarker::Period);
    assert_eq!(style.digit_group_separator, DigitGroupSeparator::Thin);
    assert!(!style.group_four_digits);
    assert_eq!(style.exponent_product, ExponentProduct::Times);
  }

  #[test]
  fn same_symbol_for_marker_and_separator_is_rejected() {
    let with = |decimal_marker, digit_group_separator| {
      return QuantityStyle {
        decimal_marker,
        digit_group_separator,
        ..QuantityStyle::default()
      };
    };

    assert!(with(DecimalMarker::Period, DigitGroupSeparator::Period).validate().is_err());
    assert!(with(DecimalMarker::Comma, DigitGroupSeparator::Comma).validate().is_err());
    assert!(with(DecimalMarker::Comma, DigitGroupSeparator::Period).validate().is_ok());
    assert!(with(DecimalMarker::Period, DigitGroupSeparator::Comma).validate().is_ok());
  }

  #[test]
  fn four_digit_grouping_without_separator_is_rejected() {
    let with = |group_four_digits| {
      return QuantityStyle {
        group_four_digits,
        digit_group_separator: DigitGroupSeparator::None,
        ..QuantityStyle::default()
      };
    };

    assert!(with(true).validate().is_err());
    assert!(with(false).validate().is_ok());
  }

  #[test]
  fn italic_typefaces_are_rejected() {
    let with = |typeface| {
      return QuantityStyle {
        typeface,
        ..QuantityStyle::default()
      };
    };

    for typeface in [
      Typeface::SerifItalic,
      Typeface::SerifBoldItalic,
      Typeface::SansSerifItalic,
      Typeface::SansSerifBoldItalic,
      Typeface::MonospaceItalic,
      Typeface::MonospaceBoldItalic,
    ] {
      assert!(with(typeface).validate().is_err(), "{typeface:?}");
    }
    for typeface in [
      Typeface::Serif,
      Typeface::SansSerifBold,
      Typeface::Monospace,
      Typeface::Math,
    ] {
      assert!(with(typeface).validate().is_ok(), "{typeface:?}");
    }
  }

  #[test]
  fn toml_keys_parse() {
    let style: QuantityStyle = toml::from_str(
      r#"
        typeface = "serif"
        decimal_marker = "comma"
        digit_group_separator = "period"
        group_four_digits = true
        exponent_product = "dot"
      "#,
    )
    .unwrap();

    assert_eq!(style.typeface, Typeface::Serif);
    assert_eq!(style.decimal_marker, DecimalMarker::Comma);
    assert_eq!(style.digit_group_separator, DigitGroupSeparator::Period);
    assert!(style.group_four_digits);
    assert_eq!(style.exponent_product, ExponentProduct::Dot);
    assert!(toml::from_str::<QuantityStyle>("unit_product = \"dot\"").is_err());
  }
}
