//! 単位付き長さ値 [`Length`]。
//!
//! 字面は config.toml / style.toml / ソースの引数で共通の `"12pt"` / `"5mm"` / `"1.5cm"`（数値と単位の間に
//! 空白なし・単位は小文字の `pt` / `mm` / `cm` のみ）で、解釈は [`FromStr`] 1 箇所。単位のない数値
//! （`12.0` や `"0"`）は受け付けない。`pt` は 1/72 inch。
//!
//! 内部表現は **sp（scaled point）= 1/65536 pt** の整数（i64）。整数加算は結合的かつ正確なので、
//! 伸縮配分や比例配分の多段加算でも誤差が蓄積せず、並列 reduce でも順序非依存でビット同一の結果を得る。
//! 浮動小数への変換は入出力境界（TOML パース・シェーパー API・PDF 座標出力・診断 / ログ / ダンプ整形）と
//! 無次元比の算出（[`Length::ratio`]）だけに閉じ、乗除（倍率・比例配分）の丸め規約は [`round_sp`] 1 箇所へ
//! 集約し、丸めと値域検査（NaN・±∞・絶対値 2^63 以上を拒否）もそこだけが行う。
//!
//! **値域の不変条件**: sp は `|sp| < 2^63`（`i64::MIN` を除く）。`i64::MIN` を除くので [`Neg`] と
//! [`Length::abs`] は失敗しない。字面の解釈は範囲外を [`ParseLengthError::OutOfRange`] で拒否し、
//! 演算 `try_*` は結果が値域外なら [`LengthOverflow`] を返す。`operators` の算術演算子は `try_*` へ
//! 委譲して溢れで落ちる、テストが期待値を組むための道具。

#![expect(
  clippy::cast_precision_loss,
  reason = "内部表現が i64 で、pt / 比率との相互変換で i64 ↔ f64 を頻繁に跨ぐ"
)]

mod operators;

use std::str::FromStr;

use derive_more::{Display, Neg};
use serde::{Deserialize, Deserializer, de::Error as _};
use thiserror::Error;

/// 1 pt あたりの sp 数（TeX の scaled point と同じ分解能 2^16）。
const SP_PER_PT: i64 = 65536;
/// 1 mm を pt に換算する係数。1 pt = 1/72 inch、1 inch = 25.4 mm。
const MM_TO_PT: f64 = 72.0 / 25.4;
/// 1 cm を pt に換算する係数。1 cm = 10 mm。
const CM_TO_PT: f64 = 10.0 * MM_TO_PT;
/// i64 の sp に収まる値の絶対値の上限 2^63（この値自身は収まらない）。
const SP_LIMIT: f64 = (1u64 << 63) as f64;
/// 字面から読める長さの絶対値の上限（pt。この値自身は読めない）。[`SP_LIMIT`] を pt にした 2^47。
pub(crate) const MAX_ABS_PT: f64 = SP_LIMIT / SP_PER_PT as f64;

/// 長さの演算結果が値域（`i64::MIN` を除く i64 の sp）に収まらないことを表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LengthOverflow;

/// sp 空間の f64 値を最近接整数（偶数丸め）へ丸める。**型内で唯一の丸め箇所**。
///
/// 半偶数（round-half-to-even）を採るのは、比例配分で `stretch * ratio` を反復して丸める際に
/// 方向性バイアスが蓄積しないようにするため。IEEE-754 準拠環境では決定的に同じ結果になる。
///
/// # Errors
///
/// NaN・±∞・絶対値が 2^63 以上の値は [`LengthOverflow`]。
#[expect(
  clippy::cast_possible_truncation,
  reason = "絶対値が 2^63 未満と検査済みで `round_ties_even` 済みの値を i64 に落とすだけ（2^63 近傍の f64 は整数なので丸めで 2^63 に達しない）"
)]
fn round_sp(sp: f64) -> Result<i64, LengthOverflow> {
  // NaN は比較が偽になるので、この形が NaN も範囲外に振り分ける
  if sp.abs() < SP_LIMIT {
    return Ok(sp.round_ties_even() as i64);
  }
  return Err(LengthOverflow);
}

/// pt 値（f64）を sp へ丸める。値域外は [`LengthOverflow`]。
fn round_to_pt_sp(pt: f64) -> Result<i64, LengthOverflow> { return round_sp(pt * SP_PER_PT as f64); }

/// 単位付き長さ値。内部は sp（1/65536 pt）の整数で保持する。
///
/// [`Display`](std::fmt::Display) の出力は [`Length::from_str`] と往復する固定の字面で、幅・寄せなどの書式パラメータは無視する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Neg, Display)]
#[display("{}pt", self.to_pt())]
pub struct Length(i64);

impl Length {
  /// 長さ 0。
  pub const ZERO: Length = Length(0);

  /// sp 値から `Length` を構築する（内部用・const 文脈用）。
  ///
  /// # Panics
  ///
  /// `sp` が `i64::MIN` のとき。値域の不変条件から `i64::MIN` を除くのは、`Neg` / `abs` を失敗しない演算に保つため。
  #[must_use]
  pub const fn from_sp(sp: i64) -> Self {
    assert!(sp != i64::MIN, "i64::MIN は Length の値域外（Neg / abs を失敗しない演算に保つため）");
    return Length(sp);
  }

  /// pt 値から `Length` を構築する（sp へ丸める）。呼び元は字面の定数だけで、実行時の値は扱わない。
  ///
  /// # Panics
  ///
  /// 値が値域（絶対値が 2^47 pt 未満）外のとき。
  #[must_use]
  pub fn pt(value: f32) -> Self {
    return Length(round_to_pt_sp(f64::from(value)).expect("字面の定数は値域内"));
  }

  /// mm 値から `Length` を構築する（pt を経由して sp へ丸める）。
  ///
  /// # Panics
  ///
  /// 値が値域外のとき（[`Length::pt`] と同じ）。
  #[must_use]
  pub fn mm(value: f32) -> Self {
    return Length(round_to_pt_sp(f64::from(value) * MM_TO_PT).expect("字面の定数は値域内"));
  }

  /// cm 値から `Length` を構築する（pt を経由して sp へ丸める）。
  ///
  /// # Panics
  ///
  /// 値が値域外のとき（[`Length::pt`] と同じ）。
  #[must_use]
  pub fn cm(value: f32) -> Self {
    return Length(round_to_pt_sp(f64::from(value) * CM_TO_PT).expect("字面の定数は値域内"));
  }

  /// 内部の sp 値を返す（比率計算などの生値取り出し用）。
  #[must_use]
  pub const fn sp(self) -> i64 { return self.0; }

  /// pt 値を f32 で返す（PDF 座標などの出力境界用）。
  #[must_use]
  #[expect(
    clippy::cast_possible_truncation,
    reason = "PDF 座標などの出力境界が f32 のため、pt 換算値をここで f32 精度へ落とすのは意図どおり"
  )]
  pub fn to_pt(self) -> f32 { return (self.0 as f64 / SP_PER_PT as f64) as f32; }

  /// pt 値を f64 で返す（mm 換算やダンプ整形など、f32 では精度が足りない変換・出力境界用）。
  #[must_use]
  pub fn to_pt_f64(self) -> f64 { return self.0 as f64 / SP_PER_PT as f64; }

  /// mm 値を f32 で返す。
  #[must_use]
  #[expect(
    clippy::cast_possible_truncation,
    reason = "`to_pt` と同じく、出力境界の f32 精度へ落とすのは意図どおり"
  )]
  pub fn to_mm(self) -> f32 { return (self.to_pt_f64() / MM_TO_PT) as f32; }

  /// 厳密に正の値か。
  #[must_use]
  pub const fn is_positive(self) -> bool { return self.0 > 0; }

  /// 非負の値か。
  #[must_use]
  pub const fn is_non_negative(self) -> bool { return self.0 >= 0; }

  /// 倍率（無次元の f64）を掛けた長さを返す。丸めは [`round_sp`] 経由。
  ///
  /// # Panics
  ///
  /// 結果が値域外のとき。
  #[must_use]
  pub fn scale(self, factor: f64) -> Self {
    return self.try_scale(factor).expect("テストが組む長さは値域内");
  }

  /// `self / denom` の無次元比を f64 で返す（伸縮の調整比・下端揃えの配分比など）。
  ///
  /// 整数オペランド同士の除算なので、IEEE-754 準拠環境では決定的に同じ結果になる。
  #[must_use]
  pub fn ratio(self, denom: Self) -> f64 { return self.0 as f64 / denom.0 as f64; }

  /// 絶対値。
  #[must_use]
  pub const fn abs(self) -> Self { return Length(self.0.abs()); }

  /// `i64::MIN` を除く i64 に収まる sp だけを `Length` にする。
  fn from_checked(sp: Option<i64>) -> Result<Self, LengthOverflow> {
    return match sp {
      Some(sp) if sp != i64::MIN => Ok(Length(sp)),
      Some(_) | None => Err(LengthOverflow),
    };
  }

  /// 和を返す。
  ///
  /// # Errors
  ///
  /// 結果が値域外のとき [`LengthOverflow`]。
  pub(crate) fn try_add(self, rhs: Self) -> Result<Self, LengthOverflow> {
    return Length::from_checked(self.0.checked_add(rhs.0));
  }

  /// 差を返す。
  ///
  /// # Errors
  ///
  /// 結果が値域外のとき [`LengthOverflow`]。
  pub(crate) fn try_sub(self, rhs: Self) -> Result<Self, LengthOverflow> {
    return Length::from_checked(self.0.checked_sub(rhs.0));
  }

  /// 倍率（無次元の f64）を掛けた長さを返す。丸めは [`round_sp`] 経由。
  ///
  /// # Errors
  ///
  /// 結果が値域外、または倍率が NaN・±∞ で積が定まらないとき [`LengthOverflow`]。
  pub(crate) fn try_scale(self, factor: f64) -> Result<Self, LengthOverflow> {
    return round_sp(self.0 as f64 * factor).map(Length);
  }

  /// 整数倍（無次元カウント）を返す。sp 整数のまま乗算するので丸め・浮動小数を経由せず厳密。
  ///
  /// # Errors
  ///
  /// 結果が値域外のとき [`LengthOverflow`]。
  pub(crate) fn try_mul(self, count: i32) -> Result<Self, LengthOverflow> {
    return Length::from_checked(self.0.checked_mul(i64::from(count)));
  }

  /// 無次元の f64 で割った長さを返す。丸めは [`round_sp`] 経由。
  ///
  /// # Errors
  ///
  /// 結果が値域外のとき、または 0 除算で ±∞ / NaN になるとき [`LengthOverflow`]。
  pub(crate) fn try_div(self, divisor: f64) -> Result<Self, LengthOverflow> {
    return round_sp(self.0 as f64 / divisor).map(Length);
  }

  /// 長さの総和を返す。最初の溢れで止める。
  ///
  /// # Errors
  ///
  /// 途中の和が値域外になったとき [`LengthOverflow`]。
  pub(crate) fn try_sum(lengths: impl IntoIterator<Item = Length>) -> Result<Self, LengthOverflow> {
    return lengths.into_iter().try_fold(Length::ZERO, Length::try_add);
  }
}

/// 単位の綴りと、1 単位あたりの pt。字面の単位は小文字のこの 3 つだけ。
const UNITS: [(&str, f64); 3] = [("pt", 1.0), ("mm", MM_TO_PT), ("cm", CM_TO_PT)];

/// [`Length`] の文字列パース失敗を表すエラー。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseLengthError {
  /// 字面が `<数値>pt` / `<数値>mm` / `<数値>cm` の書式に合わない
  #[error(
    "Length は `<数値>pt` / `<数値>mm` / `<数値>cm` のいずれかの形式（数値と単位の間に空白を入れず、単位は小文字）で指定してください: {input:?}"
  )]
  Format {
    /// パースに失敗した入力文字列。
    input: String,
  },
  /// 書式は合うが、pt へ換算した絶対値が内部表現（i64 の sp）に収まらない
  #[error(
    "長さ {input:?} は扱える範囲を超えています。pt へ換算した絶対値が {max}pt 未満になるよう値を小さくしてください",
    max = MAX_ABS_PT
  )]
  OutOfRange {
    /// パースに失敗した入力文字列。
    input: String,
  },
}

impl FromStr for Length {
  type Err = ParseLengthError;

  /// `"<数値>pt"` / `"<数値>mm"` / `"<数値>cm"` を解釈する。
  ///
  /// 前後の空白は落とすが、数値と単位の間の空白は受け付けない。
  /// `f64::from_str` は空白を含む文字列を拒否するので、数値部を `trim` しないことがその検査になる。
  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let format_error = || {
      return ParseLengthError::Format {
        input: s.to_string(),
      };
    };
    // 数値は f64 で読む。sp（1/65536pt）は f32 の仮数では表しきれず、f32 経由だとユーザ入力の
    // 数値どおりの sp にならず PDF の座標がずれる。
    let trimmed = s.trim();
    for (unit, pt_per_unit) in UNITS {
      if let Some(num) = trimmed.strip_suffix(unit) {
        let Some(parsed) = num.parse::<f64>().ok().filter(|parsed| return !parsed.is_nan()) else {
          return Err(format_error());
        };
        // `inf` の字面や `1e400` のように f64 で溢れた数値（±∞）もここで範囲外になる。
        // 2^63 近傍の f64 は整数なので、丸める前に検査しても丸めた後と同じ判定になる
        let sp = round_to_pt_sp(parsed * pt_per_unit).map_err(|LengthOverflow| {
          return ParseLengthError::OutOfRange {
            input: s.to_string(),
          };
        })?;
        return Ok(Length(sp));
      }
    }
    return Err(format_error());
  }
}

impl<'de> Deserialize<'de> for Length {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    return value.parse().map_err(D::Error::custom);
  }
}

/// `garde` 用バリデータ: [`Length`] が厳密に正の値であることを要求する。
///
/// # Errors
///
/// 値が 0 以下の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が `#[garde(custom(positive))]` の呼び出しを `&self.field` / `&()` で生成するため、値渡しへ変えると型が合わない"
)]
pub(crate) fn positive(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_positive() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("正値である必要があります（受け取った値: {value}）")));
}

/// `garde` 用バリデータ: [`Length`] が非負の値であることを要求する。
///
/// # Errors
///
/// 値が負の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "上の `positive` と同じく、garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn non_negative(value: &Length, _ctx: &()) -> garde::Result {
  if value.is_non_negative() {
    return Ok(());
  }
  return Err(garde::Error::new(format!("非負である必要があります（受け取った値: {value}）")));
}

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::{Length, LengthOverflow, ParseLengthError};

  #[derive(Debug, Deserialize)]
  struct Wrapper {
    length: Length,
  }

  #[test]
  fn parses_mm_suffix_using_inch_identity() {
    // 25.4 mm = 1 inch = 72 pt
    let w: Wrapper = toml::from_str("length = \"25.4mm\"").unwrap();

    assert!((w.length.to_pt() - 72.0).abs() < 0.01);
  }

  #[test]
  fn parses_cm_suffix_using_inch_identity() {
    // 2.54 cm = 1 inch = 72 pt
    let w: Wrapper = toml::from_str("length = \"2.54cm\"").unwrap();

    assert!((w.length.to_pt() - 72.0).abs() < 0.01);
  }

  #[test]
  fn from_str_parses_all_units() {
    assert_eq!("12pt".parse::<Length>().unwrap(), Length::pt(12.0));
    assert_eq!("  25.4mm ".parse::<Length>().unwrap(), Length::mm(25.4));
    assert_eq!("2.54cm".parse::<Length>().unwrap(), Length::cm(2.54));
  }

  #[test]
  fn from_str_rejects_unknown_unit() {
    let result = "12px".parse::<Length>();

    assert!(result.is_err());
  }

  #[test]
  fn from_str_rejects_space_between_number_and_unit() {
    // `5 pt` は `5pt` の第 2 の綴りになるので受け付けない。前後の空白は許す
    for input in ["5 pt", "5 mm", "5 cm", " 5 mm "] {
      assert!(input.parse::<Length>().is_err(), "{input:?} は拒否される");
    }
  }

  #[test]
  fn from_str_rejects_uppercase_unit() {
    for input in ["5PT", "5MM", "5Cm"] {
      assert!(input.parse::<Length>().is_err(), "{input:?} は拒否される");
    }
  }

  #[test]
  fn from_str_rejects_unitless_zero() {
    // 単位なしの `0` も例外にしない（`0` と `0pt` の 2 通りの綴りを作らない）
    assert!("0".parse::<Length>().is_err());
    assert_eq!("0pt".parse::<Length>().unwrap(), Length::ZERO);
  }

  #[test]
  fn from_str_accepts_largest_representable_lengths() {
    // 境界は単位ごとに pt へ換算した f64 で決まる（2^47 pt 未満）
    assert_eq!("140737488355327pt".parse::<Length>().unwrap().sp(), i64::MAX - 65535);
    assert_eq!("-140737488355327pt".parse::<Length>().unwrap().sp(), i64::MIN + 65536);
    for input in ["49649058392018mm", "4964905839201cm"] {
      assert!(input.parse::<Length>().is_ok(), "{input:?} は受け付ける");
    }
  }

  #[test]
  fn from_str_rejects_unrepresentable_lengths() {
    // 表せる最大値へ丸め込まず拒否する。`1e14mm` は数値こそ 2^47 未満だが pt へ換算すると超える
    for input in [
      "140737488355328pt",
      "-140737488355328pt",
      "49649058392019mm",
      "4964905839202cm",
      "1e14mm",
      "1e20pt",
      "1e400pt",
      "infpt",
    ] {
      let result = input.parse::<Length>();

      assert!(
        matches!(result, Err(ParseLengthError::OutOfRange { .. })),
        "{input:?} は範囲外として拒否される: {result:?}"
      );
    }
  }

  #[test]
  fn from_str_rejects_nan_as_format_error() {
    assert!(matches!("nanpt".parse::<Length>(), Err(ParseLengthError::Format { .. })));
  }

  #[test]
  fn try_add_rejects_sums_outside_the_representable_range() {
    let half = Length::from_sp(1i64 << 62);

    assert_eq!(Length::from_sp(i64::MAX - 1).try_add(Length::from_sp(1)), Ok(Length::from_sp(i64::MAX)));
    assert_eq!(half.try_add(half), Err(LengthOverflow));
    // i64::MIN ちょうどは値域外（Neg / abs を失敗しない演算に保つため）
    assert_eq!((-half).try_add(-half), Err(LengthOverflow));
  }

  #[test]
  fn try_sub_rejects_results_outside_the_representable_range() {
    assert_eq!(Length::from_sp(-i64::MAX).try_sub(Length::from_sp(1)), Err(LengthOverflow));
    assert_eq!(Length::from_sp(i64::MAX).try_sub(Length::from_sp(-1)), Err(LengthOverflow));
  }

  #[test]
  fn try_scale_rejects_unrepresentable_products_instead_of_saturating() {
    let base = Length::from_sp(1i64 << 52);

    assert_eq!(base.try_scale(2047.0), Ok(Length::from_sp((1i64 << 52) * 2047)));
    assert_eq!(base.try_scale(2048.0), Err(LengthOverflow));
    assert_eq!(Length::pt(12.0).try_scale(3e38), Err(LengthOverflow));
    assert_eq!(Length::pt(12.0).try_scale(f64::INFINITY), Err(LengthOverflow));
    assert_eq!(Length::pt(12.0).try_scale(f64::NAN), Err(LengthOverflow));
  }

  #[test]
  fn try_scale_and_try_div_round_half_to_even() {
    assert_eq!(Length::from_sp(3).try_scale(0.5), Ok(Length::from_sp(2)));
    assert_eq!(Length::from_sp(5).try_scale(0.5), Ok(Length::from_sp(2)));
    assert_eq!(Length::from_sp(7).try_div(2.0), Ok(Length::from_sp(4)));
  }

  #[test]
  fn try_mul_and_try_div_reject_unrepresentable_results() {
    let half = Length::from_sp(1i64 << 62);

    assert_eq!(half.try_mul(2), Err(LengthOverflow));
    assert_eq!(half.try_mul(-2), Err(LengthOverflow));
    assert_eq!(Length::from_sp(1).try_div(0.0), Err(LengthOverflow));
    assert_eq!(Length::from_sp(i64::MAX).try_div(0.5), Err(LengthOverflow));
  }

  #[test]
  fn try_sum_stops_at_the_first_overflow() {
    assert_eq!(Length::try_sum([]), Ok(Length::ZERO));
    assert_eq!(Length::try_sum([Length::pt(1.0), Length::pt(2.0)]), Ok(Length::pt(3.0)));
    assert_eq!(Length::try_sum([Length::from_sp(i64::MAX), Length::from_sp(1)]), Err(LengthOverflow));
  }

  #[test]
  #[should_panic(expected = "i64::MIN")]
  fn from_sp_rejects_i64_min() { let _ = Length::from_sp(i64::MIN); }

  #[test]
  fn display_and_from_str_round_trip() {
    let value = Length::pt(12.5);

    let text = value.to_string();

    assert_eq!(text, "12.5pt");
    assert_eq!(text.parse::<Length>().unwrap(), value);
  }

  #[test]
  fn display_ignores_formatting_parameters() {
    assert_eq!(format!("{:>8}", Length::pt(1.0)), "1pt");
  }

  #[test]
  fn rejects_bare_number() {
    let result: Result<Wrapper, _> = toml::from_str("length = 12.0");

    assert!(result.is_err());
  }

  #[test]
  fn pt_round_trips_through_sp() {
    let value = Length::pt(12.0);

    assert_eq!(value.sp(), 12 * 65536);
    assert!((value.to_pt() - 12.0).abs() < f32::EPSILON);
  }

  #[test]
  fn mul_by_integer_is_exact() {
    assert_eq!(Length::from_sp(3) * 2, Length::from_sp(6));
    assert_eq!(Length::pt(3.0) * 2, Length::pt(6.0));
  }

  #[test]
  fn div_works() {
    let a = Length::pt(6.0);
    let b = a / 2.0f32;
    assert_eq!(b, Length::pt(3.0));
  }

  #[test]
  fn scale_uses_round_ties_even() {
    // 1 sp を 0.5 倍すると 0.5 sp → 偶数丸めで 0 sp
    let one_sp = Length::from_sp(1);
    // 3 sp を 0.5 倍すると 1.5 sp → 偶数丸めで 2 sp
    let three_sp = Length::from_sp(3);

    assert_eq!(one_sp.scale(0.5), Length::from_sp(0));
    assert_eq!(three_sp.scale(0.5), Length::from_sp(2));
  }

  #[test]
  fn ratio_is_dimensionless() {
    let a = Length::pt(6.0);
    let b = Length::pt(2.0);

    assert!((a.ratio(b) - 3.0).abs() < f64::EPSILON);
  }

  #[test]
  fn sum_of_lengths_is_exact() {
    let items = [Length::pt(1.0), Length::pt(2.0), Length::pt(3.0)];

    let total: Length = items.iter().copied().sum();
    let total_ref: Length = items.iter().sum();

    assert_eq!(total, Length::pt(6.0));
    assert_eq!(total_ref, Length::pt(6.0));
  }

  #[test]
  fn from_str_error_message_names_expected_format() {
    let err = "5 pt".parse::<Length>().unwrap_err();

    assert_eq!(
      err.to_string(),
      "Length は `<数値>pt` / `<数値>mm` / `<数値>cm` のいずれかの形式（数値と単位の間に空白を入れず、単位は小文字）で指定してください: \"5 pt\""
    );
  }

  #[test]
  fn from_str_out_of_range_message_names_limit_and_fix() {
    let err = "1e20pt".parse::<Length>().unwrap_err();

    assert_eq!(
      err.to_string(),
      "長さ \"1e20pt\" は扱える範囲を超えています。pt へ換算した絶対値が 140737488355328pt 未満になるよう値を小さくしてください"
    );
  }

  #[test]
  fn deserialize_error_carries_from_str_message() {
    let err = toml::from_str::<Wrapper>("length = \"12\"").unwrap_err();

    assert!(err.to_string().contains("で指定してください: \"12\""));
  }
}
