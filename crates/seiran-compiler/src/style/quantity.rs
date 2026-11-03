//! 数値・単位・量（`\num` 等）のスタイル設定型（`[quantity]` テーブル）。

use garde::Validate;
use serde::Deserialize;
use strum::IntoStaticStr;

use crate::document::Typeface;

/// 数値・単位・量の書式と書体（`[quantity]` テーブル）
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
  /// 単位どうしの積（`\unit{N.m}`）の表記
  pub unit_product: UnitProduct,
  /// 負の指数を持つ単位（`\unit{m.s^{-1}}`）の表記
  pub negative_exponent: NegativeExponent,
  /// 量（`\qty`）の数値と単位の間のアキ。単位が平面角 ° ′ ″ の 1 因子だけ（指数なし）なら値によらず 0
  pub number_unit_separator: NumberUnitSeparator,
}

impl Default for QuantityStyle {
  fn default() -> Self {
    return Self {
      typeface: Typeface::Math,
      decimal_marker: DecimalMarker::Period,
      digit_group_separator: DigitGroupSeparator::Thin,
      group_four_digits: false,
      exponent_product: ExponentProduct::Times,
      unit_product: UnitProduct::Thin,
      negative_exponent: NegativeExponent::Power,
      number_unit_separator: NumberUnitSeparator::Thin,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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

/// 単位どうしの積の表記
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnitProduct {
  /// 細アキ（3mu）で並べる（`N m`）
  Thin,
  /// U+22C5 DOT OPERATOR を前後のアキなしで挟む（`N⋅m`）
  Dot,
}

/// 負の指数を持つ単位の表記
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NegativeExponent {
  /// 書いた順のまま負の指数で組む（`m s⁻²`）
  Power,
  /// 正の指数の因子 `/` 負の指数の因子（符号を落とす）で組む（`m/s²`）。分母の因子が 2 つ以上なら括弧で包み、正の指数の
  /// 因子が無ければ power の形のまま
  Slash,
}

/// 量の数値と単位の間のアキ
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NumberUnitSeparator {
  /// 区切らない
  None,
  /// 細アキ（3mu）
  Thin,
  /// 中アキ（4mu）
  Medium,
  /// 太アキ（5mu）
  Thick,
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
    | Typeface::MonospaceBoldItalic => Err(garde::Error::new(format!(
      "イタリック系の書体は指定できません（数値は立体で組みます。受け取った値: {}）",
      <&str>::from(*value)
    ))),
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
      return Err(garde::Error::new(format!(
        "小数点（decimal_marker）と同じ記号は指定できません（受け取った値: {}）",
        <&str>::from(*separator)
      )));
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

  use super::{
    DecimalMarker, DigitGroupSeparator, ExponentProduct, NegativeExponent, NumberUnitSeparator, QuantityStyle,
    UnitProduct,
  };
  use crate::document::Typeface;

  #[test]
  fn defaults_match_the_epic_table() {
    let style = QuantityStyle::default();

    assert_eq!(style.typeface, Typeface::Math);
    assert_eq!(style.decimal_marker, DecimalMarker::Period);
    assert_eq!(style.digit_group_separator, DigitGroupSeparator::Thin);
    assert!(!style.group_four_digits);
    assert_eq!(style.exponent_product, ExponentProduct::Times);
    assert_eq!(style.unit_product, UnitProduct::Thin);
    assert_eq!(style.negative_exponent, NegativeExponent::Power);
    assert_eq!(style.number_unit_separator, NumberUnitSeparator::Thin);
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
  fn violation_messages_carry_the_received_value() {
    let italic = QuantityStyle {
      typeface: Typeface::SansSerifItalic,
      ..QuantityStyle::default()
    };
    let same = QuantityStyle {
      decimal_marker: DecimalMarker::Comma,
      digit_group_separator: DigitGroupSeparator::Comma,
      ..QuantityStyle::default()
    };

    assert!(italic.validate().unwrap_err().to_string().contains("sans_serif_italic"), "{:?}", italic.validate());
    assert!(same.validate().unwrap_err().to_string().contains("comma"), "{:?}", same.validate());
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
        unit_product = "dot"
        negative_exponent = "slash"
        number_unit_separator = "thick"
      "#,
    )
    .unwrap();

    assert_eq!(style.typeface, Typeface::Serif);
    assert_eq!(style.decimal_marker, DecimalMarker::Comma);
    assert_eq!(style.digit_group_separator, DigitGroupSeparator::Period);
    assert!(style.group_four_digits);
    assert_eq!(style.exponent_product, ExponentProduct::Dot);
    assert_eq!(style.unit_product, UnitProduct::Dot);
    assert_eq!(style.negative_exponent, NegativeExponent::Slash);
    assert_eq!(style.number_unit_separator, NumberUnitSeparator::Thick);
    assert!(toml::from_str::<QuantityStyle>("unknown_key = 1").is_err());
  }
}
