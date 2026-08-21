//! 単位付き長さ値 [`Length`]。
//!
//! TOML 上では `"12pt"` / `"5mm"` / `"1.5cm"` のいずれかの文字列で指定する。
//! 素の数値（`12.0` のような）は受け付けない。
//!
//! 内部表現は **sp（scaled point）= 1/65536 pt** の整数（i64）。整数加算は結合的かつ正確なので、
//! 伸縮配分や比例配分の多段加算でも誤差が蓄積せず、並列 reduce でも順序非依存でビット同一の結果を得る。
//! 浮動小数への変換は入出力境界（TOML パース・PDF 座標出力・ダンプ整形）だけに閉じ、乗除（倍率・比例
//! 配分）の丸め規約は [`round_sp`] 1 箇所へ集約する。i64 の値域は ±1.4e14 pt と実質無限のため、
//! オーバーフロー・飽和・上限（`\maxdimen` 相当）といった境界処理は一切設けない。
//!
//! 各スタイル構造体の `font_size` / `bottom_margin` などはこの型を用い、`garde` の `custom`
//! バリデータ [`positive`] / [`non_negative`] で 0 や負値を弾く。

#![allow(
  clippy::cast_precision_loss,
  reason = "内部表現が i64 で、pt / 比率との相互変換で i64 ↔ f64 を頻繁に跨ぐ"
)]

use std::{
  fmt,
  iter::Sum,
  ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign},
  str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

/// 1 pt あたりの sp 数（TeX の scaled point と同じ分解能 2^16）。
const SP_PER_PT: i64 = 65536;
/// 1 mm を pt に換算する係数。1 pt = 1/72 inch、1 inch = 25.4 mm。
const MM_TO_PT: f64 = 72.0 / 25.4;
/// 1 cm を pt に換算する係数。1 cm = 10 mm。
const CM_TO_PT: f64 = 10.0 * MM_TO_PT;

/// sp 空間の f64 値を最近接整数（偶数丸め）へ丸める。**型内で唯一の丸め箇所**。
///
/// 半偶数（round-half-to-even）を採るのは、比例配分で `stretch * ratio` を反復して丸める際に
/// 方向性バイアスが蓄積しないようにするため。IEEE-754 準拠環境では決定的に同じ結果になる。
#[allow(
  clippy::cast_possible_truncation,
  reason = "`round_ties_even` 済みの値を i64 に落とすだけで端数は残らない（型内で唯一の丸め箇所）"
)]
fn round_sp(sp: f64) -> i64 { return sp.round_ties_even() as i64; }

/// pt 値（f64）を sp へ丸める。
fn round_to_pt_sp(pt: f64) -> i64 { return round_sp(pt * SP_PER_PT as f64); }

/// 単位付き長さ値。内部は sp（1/65536 pt）の整数で保持する。
///
/// 構築は [`Length::pt`] / [`Length::mm`] / [`Length::from_sp`]、pt 値の取り出しは [`Length::to_pt`]。
/// 文字列との相互変換は [`FromStr`] / [`Display`](fmt::Display) の正準形 `<pt値>pt` を用いる。
/// `Deref` / `From<f32>` は意図的に実装しない（変換漏れを型検査で検出するため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Length(i64);

impl Length {
  /// 長さ 0。
  pub const ZERO: Length = Length(0);

  /// sp 値から `Length` を構築する（内部用・const 文脈用）。
  #[must_use]
  pub const fn from_sp(sp: i64) -> Self { return Length(sp); }

  /// pt 値から `Length` を構築する（sp へ丸める）。
  #[must_use]
  pub fn pt(value: f32) -> Self { return Length(round_to_pt_sp(f64::from(value))); }

  /// mm 値から `Length` を構築する（pt を経由して sp へ丸める）。
  #[must_use]
  pub fn mm(value: f32) -> Self { return Length(round_to_pt_sp(f64::from(value) * MM_TO_PT)); }

  /// cm 値から `Length` を構築する（pt を経由して sp へ丸める）。
  #[must_use]
  pub fn cm(value: f32) -> Self { return Length(round_to_pt_sp(f64::from(value) * CM_TO_PT)); }

  /// 内部の sp 値を返す（比率計算などの生値取り出し用）。
  #[must_use]
  #[allow(dead_code, reason = "crate 内の `#[cfg(test)]` からのみ使う")]
  pub const fn sp(self) -> i64 { return self.0; }

  /// pt 値を f32 で返す（PDF 座標などの出力境界用）。
  #[must_use]
  #[allow(
    clippy::cast_possible_truncation,
    reason = "PDF 座標などの出力境界が f32 のため、pt 換算値をここで f32 精度へ落とすのは意図どおり"
  )]
  pub fn to_pt(self) -> f32 { return (self.0 as f64 / SP_PER_PT as f64) as f32; }

  /// pt 値を f64 で返す（[`Serialize`] の正準形やダンプ整形など、高精度が要る出力境界用）。
  #[must_use]
  pub fn to_pt_f64(self) -> f64 { return self.0 as f64 / SP_PER_PT as f64; }

  /// mm 値を f32 で返す。
  #[must_use]
  #[allow(
    clippy::cast_possible_truncation,
    reason = "`to_pt` と同じく、出力境界の f32 精度へ落とすのは意図どおり"
  )]
  #[allow(dead_code, reason = "crate 内の `#[cfg(test)]` からのみ使う")]
  pub fn to_mm(self) -> f32 { return (self.to_pt_f64() / MM_TO_PT) as f32; }

  /// 厳密に正の値か。
  #[must_use]
  pub const fn is_positive(self) -> bool { return self.0 > 0; }

  /// 非負の値か。
  #[must_use]
  pub const fn is_non_negative(self) -> bool { return self.0 >= 0; }

  /// 倍率（無次元の f64）を掛けた長さを返す。丸めは [`round_sp`] 経由。
  #[must_use]
  pub fn scale(self, factor: f64) -> Self { return Length(round_sp(self.0 as f64 * factor)); }

  /// `self / denom` の無次元比を f64 で返す（伸縮の調整比・下端揃えの配分比など）。
  ///
  /// 整数オペランド同士の除算なので、IEEE-754 準拠環境では決定的に同じ結果になる。
  #[must_use]
  pub fn ratio(self, denom: Self) -> f64 { return self.0 as f64 / denom.0 as f64; }

  /// 絶対値。
  #[must_use]
  #[allow(dead_code, reason = "crate 内の `#[cfg(test)]` からのみ使う")]
  pub const fn abs(self) -> Self { return Length(self.0.abs()); }
}

/// `"<数値>pt"` / `"<数値>mm"` / `"<数値>cm"` を解釈する。失敗時は `None`。
fn parse_length(value: &str) -> Option<Length> {
  // 数値は f64 で読む。sp（1/65536pt）は f32 の仮数では表しきれず、f32 経由だと
  // `Serialize` が書き出した正準形を読み戻したときに最大数 sp ずれる（往復が壊れる）。
  let trimmed = value.trim();
  if let Some(num) = trimmed.strip_suffix("pt") {
    let parsed: f64 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length(round_to_pt_sp(parsed)));
  }
  if let Some(num) = trimmed.strip_suffix("mm") {
    let parsed: f64 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length(round_to_pt_sp(parsed * MM_TO_PT)));
  }
  if let Some(num) = trimmed.strip_suffix("cm") {
    let parsed: f64 = num.trim().parse().ok()?;
    if !parsed.is_finite() {
      return None;
    }
    return Some(Length(round_to_pt_sp(parsed * CM_TO_PT)));
  }
  return None;
}

/// [`Length`] の文字列パース失敗を表すエラー。
///
/// `<数値>pt` / `<数値>mm` / `<数値>cm` 以外の形式で [`Length::from_str`] が返す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLengthError {
  /// パースに失敗した入力文字列。
  input: String,
}

impl fmt::Display for ParseLengthError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return write!(
      f,
      "Length は `<数値>pt` / `<数値>mm` / `<数値>cm` のいずれかの形式で指定してください: {:?}",
      self.input
    );
  }
}

impl std::error::Error for ParseLengthError {}

impl FromStr for Length {
  type Err = ParseLengthError;

  /// `"<数値>pt"` / `"<数値>mm"` / `"<数値>cm"` を解釈する。前後の空白は許容する。
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    return parse_length(s).ok_or_else(|| {
      return ParseLengthError {
        input: s.to_string(),
      };
    });
  }
}

impl fmt::Display for Length {
  /// 正準の人間可読表現 `<pt値>pt`。[`Length::from_str`] と往復する。
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { return write!(f, "{}pt", self.to_pt()); }
}

impl<'de> Deserialize<'de> for Length {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    return value.parse().map_err(D::Error::custom);
  }
}

impl Serialize for Length {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // 内部表現は sp なので pt に戻して pt サフィックス付きの正準形で書き出す。
    // 人間向けの [`Display`](fmt::Display)（f32）ではなく f64 で書くのは、`Deserialize` と
    // 無損失に往復させるため（sp は f32 の仮数に収まらない）。
    return serializer.serialize_str(&format!("{}pt", self.to_pt_f64()));
  }
}

/// `garde` 用バリデータ: [`Length`] が厳密に正の値であることを要求する。
///
/// `#[garde(custom(positive))]` で各フィールドに付ける。
///
/// # Errors
///
/// 値が 0 以下の場合に [`garde::Error`] を返す。
#[allow(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が `#[garde(custom(positive))]` の呼び出しを `&self.field` / `&()` で生成するため、値渡しへ変えると型が合わない（#307 で E0308 が 15 件出ることを確認済み）"
)]
pub(crate) fn positive(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_positive() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("正値である必要があります（受け取った値: {value}）")));
}

/// `garde` 用バリデータ: [`Length`] が非負の値であることを要求する。
///
/// `#[garde(custom(non_negative))]` で各フィールドに付ける。
///
/// # Errors
///
/// 値が負の場合に [`garde::Error`] を返す。
#[allow(
  clippy::trivially_copy_pass_by_ref,
  reason = "上の `positive` と同じく、garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn non_negative(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_non_negative() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("非負である必要があります（受け取った値: {value}）")));
}

impl Add for Length {
  type Output = Self;

  fn add(self, rhs: Self) -> Self::Output { return Length(self.0 + rhs.0); }
}

impl Sub for Length {
  type Output = Self;

  fn sub(self, rhs: Self) -> Self::Output { return Length(self.0 - rhs.0); }
}

impl Neg for Length {
  type Output = Self;

  fn neg(self) -> Self::Output { return Length(-self.0); }
}

impl AddAssign for Length {
  fn add_assign(&mut self, rhs: Self) { self.0 += rhs.0; }
}

impl SubAssign for Length {
  fn sub_assign(&mut self, rhs: Self) { self.0 -= rhs.0; }
}

impl Mul<f64> for Length {
  type Output = Self;

  fn mul(self, rhs: f64) -> Self::Output { return self.scale(rhs); }
}

impl Mul<f32> for Length {
  type Output = Self;

  fn mul(self, rhs: f32) -> Self::Output { return self.scale(f64::from(rhs)); }
}

impl Mul<i32> for Length {
  type Output = Self;

  /// 整数倍（無次元カウント）。sp 整数のまま乗算するので丸め・浮動小数を経由せず厳密。
  /// 整数リテラルは既定で `i32` に解決されるため受け側も `i32` とする。
  fn mul(self, rhs: i32) -> Self::Output { return Length(self.0 * i64::from(rhs)); }
}

impl Div<f64> for Length {
  type Output = Self;

  fn div(self, rhs: f64) -> Self::Output { return Length(round_sp(self.0 as f64 / rhs)); }
}

impl Div<f32> for Length {
  type Output = Self;

  fn div(self, rhs: f32) -> Self::Output { return Length(round_sp(self.0 as f64 / f64::from(rhs))); }
}

impl Sum for Length {
  fn sum<I: Iterator<Item = Self>>(iter: I) -> Self { return iter.fold(Length::ZERO, |acc, x| return acc + x); }
}

impl<'a> Sum<&'a Length> for Length {
  fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self { return iter.fold(Length::ZERO, |acc, x| return acc + *x); }
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};

  use super::{Length, non_negative, positive};

  #[derive(Debug, Deserialize, Serialize)]
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
    // Arrange / Act: 1 cm == 10 mm（sp 整数として厳密一致）
    let a: Wrapper = toml::from_str("length = \"1cm\"").unwrap();
    let b: Wrapper = toml::from_str("length = \"10mm\"").unwrap();

    // Assert
    assert_eq!(a.length, b.length);
  }

  #[test]
  fn parses_decimal_value() {
    // Arrange / Act
    let w: Wrapper = toml::from_str("length = \"0.5pt\"").unwrap();

    // Assert
    assert!((w.length.to_pt() - 0.5).abs() < f32::EPSILON);
  }

  #[test]
  fn from_str_parses_all_units() {
    // Arrange / Act / Assert
    assert_eq!("12pt".parse::<Length>().unwrap(), Length::pt(12.0));
    assert_eq!("  25.4mm ".parse::<Length>().unwrap(), Length::mm(25.4));
    assert_eq!("2.54cm".parse::<Length>().unwrap(), Length::cm(2.54));
  }

  #[test]
  fn from_str_rejects_unknown_unit() {
    // Arrange / Act
    let result = "12px".parse::<Length>();

    // Assert
    assert!(result.is_err());
  }

  #[test]
  fn display_and_from_str_round_trip() {
    // Arrange
    let value = Length::pt(12.5);

    // Act: Display の正準形 `<pt>pt` を FromStr で往復
    let text = value.to_string();

    // Assert
    assert_eq!(text, "12.5pt");
    assert_eq!(text.parse::<Length>().unwrap(), value);
  }

  #[test]
  fn serde_round_trip_is_lossless_for_mm_values() {
    // Arrange — mm 由来の値は sp が f32 の仮数に収まらず、f32 経由の往復では数 sp ずれる。
    // style.toml を型付き `Style` から再直列化して読み直す経路（golden）が座標を動かさないよう、
    // serde の往復は無損失であることを固定する。
    for mm in [99.0_f32, 85.0, 275.0, 12.0, 0.5] {
      let value = Length::mm(mm);

      // Act
      let text = toml::to_string(&Wrapper { length: value }).expect("Length を TOML へ書けるはず");
      let parsed: Wrapper = toml::from_str(&text).expect("書き出した TOML を読み戻せるはず");

      // Assert
      assert_eq!(parsed.length, value, "{mm}mm の serde 往復で値が変わった: {text}");
    }
  }

  #[test]
  fn rejects_bare_number() {
    // Arrange / Act
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
  fn pt_round_trips_through_sp() {
    // Arrange: 12pt = 12 * 65536 sp = 786432 sp
    let value = Length::pt(12.0);

    // Assert
    assert_eq!(value.sp(), 12 * 65536);
    assert!((value.to_pt() - 12.0).abs() < f32::EPSILON);
  }

  #[test]
  fn zero_is_additive_identity() {
    // Arrange
    let a = Length::pt(7.0);

    // Assert
    assert_eq!(a + Length::ZERO, a);
    assert_eq!(Length::ZERO.sp(), 0);
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

  #[test]
  fn add_works() {
    let a = Length::pt(1.0);
    let b = Length::pt(2.0);
    let c = a + b;
    assert_eq!(c, Length::pt(3.0));
  }

  #[test]
  fn sub_works() {
    let a = Length::pt(5.0);
    let b = Length::pt(3.0);
    let c = a - b;
    assert_eq!(c, Length::pt(2.0));
  }

  #[test]
  fn neg_works() {
    let a = Length::pt(4.0);
    assert_eq!(-a, Length::pt(-4.0));
  }

  #[test]
  fn add_assign_and_sub_assign_work() {
    let mut a = Length::pt(1.0);
    a += Length::pt(2.0);
    assert_eq!(a, Length::pt(3.0));
    a -= Length::pt(1.0);
    assert_eq!(a, Length::pt(2.0));
  }

  #[test]
  fn mul_works() {
    let a = Length::pt(2.0);
    let b = a * 3.0_f32;
    assert_eq!(b, Length::pt(6.0));
  }

  #[test]
  fn mul_by_integer_is_exact() {
    // Arrange / Act / Assert
    assert_eq!(Length::from_sp(3) * 2, Length::from_sp(6));
    assert_eq!(Length::pt(3.0) * 2, Length::pt(6.0));
  }

  #[test]
  fn div_works() {
    let a = Length::pt(6.0);
    let b = a / 2.0_f32;
    assert_eq!(b, Length::pt(3.0));
  }

  #[test]
  fn scale_rounds_through_single_site() {
    // Arrange: 3pt = 196608 sp、× 0.5 = 98304 sp = 1.5pt
    let a = Length::pt(3.0);

    // Assert
    assert_eq!(a.scale(0.5), Length::pt(1.5));
  }

  #[test]
  fn scale_uses_round_ties_even() {
    // Arrange: 1 sp を 0.5 倍すると 0.5 sp → 偶数丸めで 0 sp
    let one_sp = Length::from_sp(1);
    // 3 sp を 0.5 倍すると 1.5 sp → 偶数丸めで 2 sp
    let three_sp = Length::from_sp(3);

    // Assert
    assert_eq!(one_sp.scale(0.5), Length::from_sp(0));
    assert_eq!(three_sp.scale(0.5), Length::from_sp(2));
  }

  #[test]
  fn ratio_is_dimensionless() {
    // Arrange
    let a = Length::pt(6.0);
    let b = Length::pt(2.0);

    // Assert
    assert!((a.ratio(b) - 3.0).abs() < f64::EPSILON);
  }

  #[test]
  fn min_max_abs_work() {
    let a = Length::pt(2.0);
    let b = Length::pt(5.0);
    assert_eq!(a.min(b), a);
    assert_eq!(a.max(b), b);
    assert_eq!(Length::pt(-3.0).abs(), Length::pt(3.0));
  }

  #[test]
  fn sum_of_lengths_is_exact() {
    // Arrange
    let items = [Length::pt(1.0), Length::pt(2.0), Length::pt(3.0)];

    // Act
    let total: Length = items.iter().copied().sum();
    let total_ref: Length = items.iter().sum();

    // Assert
    assert_eq!(total, Length::pt(6.0));
    assert_eq!(total_ref, Length::pt(6.0));
  }
}
