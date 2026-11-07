//! テストが期待値を組むための算術演算子。本体は `try_*` で溢れを検査する。

use std::{
  iter::Sum,
  ops::{Add, AddAssign, Div, Mul, Sub, SubAssign},
};

use crate::length::Length;

impl Add for Length {
  type Output = Self;

  fn add(self, rhs: Self) -> Self::Output { return self.try_add(rhs).expect("テストが組む長さは値域内"); }
}

impl Sub for Length {
  type Output = Self;

  fn sub(self, rhs: Self) -> Self::Output { return self.try_sub(rhs).expect("テストが組む長さは値域内"); }
}

impl AddAssign for Length {
  fn add_assign(&mut self, rhs: Self) { *self = *self + rhs; }
}

impl SubAssign for Length {
  fn sub_assign(&mut self, rhs: Self) { *self = *self - rhs; }
}

impl Mul<f64> for Length {
  type Output = Self;

  fn mul(self, rhs: f64) -> Self::Output { return self.try_scale(rhs).expect("テストが組む長さは値域内"); }
}

impl Mul<f32> for Length {
  type Output = Self;

  fn mul(self, rhs: f32) -> Self::Output {
    return self.try_scale(f64::from(rhs)).expect("テストが組む長さは値域内");
  }
}

impl Mul<i32> for Length {
  type Output = Self;

  fn mul(self, rhs: i32) -> Self::Output { return self.try_mul(rhs).expect("テストが組む長さは値域内"); }
}

impl Div<f64> for Length {
  type Output = Self;

  fn div(self, rhs: f64) -> Self::Output { return self.try_div(rhs).expect("テストが組む長さは値域内"); }
}

impl Div<f32> for Length {
  type Output = Self;

  fn div(self, rhs: f32) -> Self::Output {
    return self.try_div(f64::from(rhs)).expect("テストが組む長さは値域内");
  }
}

impl Sum for Length {
  fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
    return Length::try_sum(iter).expect("テストが組む長さは値域内");
  }
}

impl<'a> Sum<&'a Length> for Length {
  fn sum<I: Iterator<Item = &'a Self>>(iter: I) -> Self {
    return Length::try_sum(iter.copied()).expect("テストが組む長さは値域内");
  }
}
