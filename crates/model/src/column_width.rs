//! 段組みの 1 段あたりの幅を求める純粋計算
//!
//! `config`（用紙・余白 × `[columns]` の横断バリデーション）と `typeset::breaking`（実際の段組み
//! 配置）の双方がこの式を必要とするため、両クレートが依存できる `model` に置く。

use crate::Length;

/// 本文幅 `text_width` を `num_columns` 段に分けたときの 1 段あたりの幅（pt）を返す。
///
/// `(text_width - (num_columns - 1) * column_gap) / num_columns`。`config::validate_layout`・
/// `build_pdf` の `resolve_images`・`typeset::breaking::break_pages` がこの 1 つの式を真実源として
/// 段幅を求める（重複した算出を避ける）。
#[must_use]
pub fn column_width(text_width: Length, num_columns: usize, column_gap: Length) -> Length {
  let count = num_columns.max(1);
  // 段数は実用上 1〜2。桁あふれ・精度低下する桁数にはならない
  #[allow(clippy::cast_precision_loss)]
  let n = count as f32;
  #[allow(clippy::cast_possible_wrap)]
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
