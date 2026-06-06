//! 単位付き長さ値 [`Length`]。
//!
//! TOML 上では `"12pt"` / `"5mm"` / `"1.5cm"` のいずれかの文字列で指定する。
//! 素の数値（`12.0` のような）は受け付けない。内部表現は常に pt（ポイント）。
//!
//! 各スタイル構造体の `font_size` / `bottom_margin` などはこの型を用い、`garde` の `custom`
//! バリデータ [`positive`] / [`non_negative`] で 0 や負値を弾く。

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

/// 1 mm を pt に換算する係数。1 pt = 1/72 inch、1 inch = 25.4 mm。
const MM_TO_PT: f32 = 72.0 / 25.4;
/// 1 cm を pt に換算する係数。1 cm = 10 mm。
const CM_TO_PT: f32 = 10.0 * MM_TO_PT;

/// 単位付き長さ値。内部は pt（ポイント）で保持する。
///
/// 構築は [`Length::pt`] / [`Length::mm`]、pt 値の取り出しは [`Length::to_pt`]。
/// `Deref` / `From<f32>` は意図的に実装しない（変換漏れを型検査で検出するため）。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Length(f32);

impl Length {
  /// pt 値から `Length` を構築する。
  #[must_use]
  pub const fn pt(value: f32) -> Self { return Length(value); }

  /// mm 値から `Length` を構築する（内部で pt に換算）。
  #[must_use]
  pub fn mm(value: f32) -> Self { return Length(value * MM_TO_PT); }

  /// cm 値から `Length` を構築する（内部で pt に換算）。
  #[must_use]
  pub fn cm(value: f32) -> Self { return Length(value * CM_TO_PT); }

  /// 内部の pt 値を返す。
  #[must_use]
  pub const fn to_pt(self) -> f32 { return self.0; }

  /// 厳密に正の値か。
  #[must_use]
  pub fn is_positive(self) -> bool { return self.0 > 0.0; }

  /// 非負の値か。
  #[must_use]
  pub fn is_non_negative(self) -> bool { return self.0 >= 0.0; }
}

/// `"<数値>pt"` / `"<数値>mm"` / `"<数値>cm"` を解釈する。失敗時は `None`。
fn parse_length(value: &str) -> Option<Length> {
  let trimmed = value.trim();
  if let Some(num) = trimmed.strip_suffix("pt") {
    let parsed: f32 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length::pt(parsed));
  }
  if let Some(num) = trimmed.strip_suffix("mm") {
    let parsed: f32 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length::mm(parsed));
  }
  if let Some(num) = trimmed.strip_suffix("cm") {
    let parsed: f32 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length::cm(parsed));
  }
  return None;
}

impl<'de> Deserialize<'de> for Length {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    return parse_length(&value).ok_or_else(|| {
      D::Error::custom(format!(
        "Length は `<数値>pt` / `<数値>mm` / `<数値>cm` のいずれかの形式で指定してください: {value:?}"
      ))
    });
  }
}

impl Serialize for Length {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // 内部表現は常に pt なので pt サフィックスで書き出す
    return format!("{}pt", self.0).serialize(serializer);
  }
}

/// `garde` 用バリデータ: [`Length`] が厳密に正の値であることを要求する。
///
/// `#[garde(custom(positive))]` で各フィールドに付ける。
///
/// # Errors
///
/// 値が 0 以下の場合に [`garde::Error`] を返す。
pub fn positive(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_positive() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("正値である必要があります（受け取った値: {}pt）", value.to_pt())));
}

/// `garde` 用バリデータ: [`Length`] が非負の値であることを要求する。
///
/// `#[garde(custom(non_negative))]` で各フィールドに付ける。
///
/// # Errors
///
/// 値が負の場合に [`garde::Error`] を返す。
pub fn non_negative(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_non_negative() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("非負である必要があります（受け取った値: {}pt）", value.to_pt())));
}

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::{Length, non_negative, positive};

  #[derive(Debug, Deserialize)]
  struct Wrapper {
    length: Length,
  }

  #[test]
  fn parses_pt_suffix() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("length = \"12pt\"").unwrap();

    // Assert
    assert!((w.length.to_pt() - 12.0).abs() < f32::EPSILON);
  }

  #[test]
  fn parses_mm_suffix_using_inch_identity() {
    // Arrange: 25.4 mm = 1 inch = 72 pt
    let w: Wrapper = toml::from_str("length = \"25.4mm\"").unwrap();

    // Assert
    assert!((w.length.to_pt() - 72.0).abs() < 0.01);
  }

  #[test]
  fn parses_cm_suffix_using_inch_identity() {
    // Arrange: 2.54 cm = 1 inch = 72 pt
    let w: Wrapper = toml::from_str("length = \"2.54cm\"").unwrap();

    // Assert
    assert!((w.length.to_pt() - 72.0).abs() < 0.01);
  }

  #[test]
  fn cm_and_mm_are_consistent() {
    // Arrange / Act: 1 cm == 10 mm
    let a: Wrapper = toml::from_str("length = \"1cm\"").unwrap();
    let b: Wrapper = toml::from_str("length = \"10mm\"").unwrap();

    // Assert
    assert!((a.length.to_pt() - b.length.to_pt()).abs() < f32::EPSILON);
  }

  #[test]
  fn parses_decimal_value() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("length = \"0.5pt\"").unwrap();

    // Assert
    assert!((w.length.to_pt() - 0.5).abs() < f32::EPSILON);
  }

  #[test]
  fn rejects_bare_number() {
    // Arrange: 単位なしの素 f32 は拒否
    let result: Result<Wrapper, _> = toml::from_str("length = 12.0");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn rejects_unknown_unit() {
    // Arrange
    let result: Result<Wrapper, _> = toml::from_str("length = \"12px\"");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn rejects_missing_unit() {
    // Arrange
    let result: Result<Wrapper, _> = toml::from_str("length = \"12\"");

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn positive_validator_accepts_positive() {
    assert!(positive(&Length::pt(1.0), &()).is_ok());
  }

  #[test]
  fn positive_validator_rejects_zero_and_negative() {
    assert!(positive(&Length::pt(0.0), &()).is_err());
    assert!(positive(&Length::pt(-1.0), &()).is_err());
  }

  #[test]
  fn non_negative_validator_accepts_zero() {
    assert!(non_negative(&Length::pt(0.0), &()).is_ok());
    assert!(non_negative(&Length::pt(1.0), &()).is_ok());
  }

  #[test]
  fn non_negative_validator_rejects_negative() {
    assert!(non_negative(&Length::pt(-0.1), &()).is_err());
  }
}
