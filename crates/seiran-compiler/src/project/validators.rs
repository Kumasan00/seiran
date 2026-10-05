//! garde の `custom` に渡す、文字列・配列・数値の設定値検証器。
//!
//! 違反文言は日本語で、数値の違反には受け取った値を載せる（`*ValidationError::Field` の `message` がそのまま
//! 診断の本文になる）。`Length` の検証器は `length` が持つ。

use std::fmt::Display;

/// `garde` 用バリデータ: 文字列が空でないことを要求する。
///
/// # Errors
///
/// 空文字列の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn non_empty_text(value: &str, _ctx: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("空文字列は指定できません"));
  }
  return Ok(());
}

/// `garde` 用バリデータ: 配列が 1 要素以上であることを要求する。
///
/// # Errors
///
/// 空配列の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn non_empty_list<T>(value: &[T], _ctx: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("空の配列は指定できません"));
  }
  return Ok(());
}

/// `garde` 用バリデータを返す: 値が `min` 以上 `max` 以下（両端を含む）であることを要求する。
///
/// 返す検証器は、範囲外の値に対して境界と受け取った値を載せた [`garde::Error`] を返す。
pub(crate) fn in_range<T: PartialOrd + Display>(min: T, max: T) -> impl FnOnce(&T, &()) -> garde::Result {
  return move |value, _ctx| {
    if *value < min || *value > max {
      return Err(garde::Error::new(format!("{min} 以上 {max} 以下である必要があります（受け取った値: {value}）")));
    }
    return Ok(());
  };
}

/// `garde` 用バリデータ: `f32` が正の有限値であることを要求する。
///
/// # Errors
///
/// 値が 0 以下・無限大・NaN の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn positive_finite(value: &f32, _ctx: &()) -> garde::Result {
  if value.is_finite() && *value > 0.0 {
    return Ok(());
  }
  return Err(garde::Error::new(format!("正の有限値である必要があります（受け取った値: {value}）")));
}

/// `garde` 用バリデータ: `f32` が非負の有限値であることを要求する。
///
/// # Errors
///
/// 値が負・無限大・NaN の場合に [`garde::Error`] を返す。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
pub(crate) fn non_negative_finite(value: &f32, _ctx: &()) -> garde::Result {
  if value.is_finite() && *value >= 0.0 {
    return Ok(());
  }
  return Err(garde::Error::new(format!("非負の有限値である必要があります（受け取った値: {value}）")));
}

#[cfg(test)]
mod tests {
  use super::{in_range, non_empty_list, non_empty_text, non_negative_finite, positive_finite};

  #[test]
  fn non_empty_text_rejects_empty() {
    let error = non_empty_text("", &()).unwrap_err();
    assert_eq!(error.to_string(), "空文字列は指定できません");
  }

  #[test]
  fn non_empty_text_accepts_single_multibyte_char() {
    assert!(non_empty_text("•", &()).is_ok());
  }

  #[test]
  fn non_empty_list_rejects_empty() {
    let error = non_empty_list::<String>(&[], &()).unwrap_err();
    assert_eq!(error.to_string(), "空の配列は指定できません");
  }

  #[test]
  fn non_empty_list_accepts_one_element() {
    assert!(non_empty_list(&[String::new()], &()).is_ok());
  }

  #[test]
  fn in_range_accepts_both_bounds() {
    assert!(in_range(1u32, 2400u32)(&1u32, &()).is_ok());
    assert!(in_range(1u32, 2400u32)(&2400u32, &()).is_ok());
  }

  #[test]
  fn in_range_reports_bounds_and_received_value() {
    let above = in_range(1u32, 2400u32)(&9999u32, &()).unwrap_err();
    let below = in_range(1u8, 3u8)(&0u8, &()).unwrap_err();

    assert_eq!(above.to_string(), "1 以上 2400 以下である必要があります（受け取った値: 9999）");
    assert_eq!(below.to_string(), "1 以上 3 以下である必要があります（受け取った値: 0）");
  }

  #[test]
  fn positive_finite_accepts_positive_and_rejects_zero() {
    assert!(positive_finite(&1.2f32, &()).is_ok());
    let error = positive_finite(&0.0f32, &()).unwrap_err();
    assert_eq!(error.to_string(), "正の有限値である必要があります（受け取った値: 0）");
  }

  #[test]
  fn positive_finite_rejects_nan_and_infinity() {
    // NaN はどの大小比較も偽になるので、上下限の比較だけでは素通りする
    let nan = positive_finite(&f32::NAN, &()).unwrap_err();
    let infinity = positive_finite(&f32::INFINITY, &()).unwrap_err();

    assert_eq!(nan.to_string(), "正の有限値である必要があります（受け取った値: NaN）");
    assert_eq!(infinity.to_string(), "正の有限値である必要があります（受け取った値: inf）");
  }

  #[test]
  fn non_negative_finite_accepts_zero_and_rejects_negative_and_nan() {
    assert!(non_negative_finite(&0.0f32, &()).is_ok());
    let negative = non_negative_finite(&-0.5f32, &()).unwrap_err();
    let nan = non_negative_finite(&f32::NAN, &()).unwrap_err();

    assert_eq!(negative.to_string(), "非負の有限値である必要があります（受け取った値: -0.5）");
    assert_eq!(nan.to_string(), "非負の有限値である必要があります（受け取った値: NaN）");
  }
}
