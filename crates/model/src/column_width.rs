//! 段組みの 1 段あたりの幅を求める計算。

use crate::Length;

/// 本文幅 `text_width` を `num_columns` 段に分けたときの 1 段あたりの幅（pt）を返す。
///
/// `(text_width - (num_columns - 1) * column_gap) / num_columns`。`config::validate_layout`・
/// `build_pdf`・`typeset::breaking` が共通して使用する。
#[must_use]
pub fn column_width(text_width: Length, num_columns: usize, column_gap: Length) -> Length {
  let count = num_columns.max(1);
  // 段数は実用上 1〜2。桁あふれ・精度低下・切り捨てが起きる桁数にはならない
  #[allow(clippy::cast_precision_loss)]
  let n = count as f32;
  #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
  let gaps = (count - 1) as i32;
  return (text_width - column_gap * gaps) / n;
}

#[cfg(test)]
mod tests {
  use super::column_width;
  use crate::Length;

  fn pt(value: f32) -> Length { return Length::pt(value); }

  fn close(a: Length, b: f32) -> bool { return (a.to_pt() - b).abs() < 0.01; }

  #[test]
  fn column_width_helper_divides_text_width() {
    assert!(close(column_width(pt(100.0), 2, pt(10.0)), 45.0));
    assert!(close(column_width(pt(100.0), 1, pt(18.0)), 100.0));
  }
}
