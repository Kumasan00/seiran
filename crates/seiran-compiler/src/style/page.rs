//! ページ組版の挙動スタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::length::{Length, non_negative};

/// ページ組版の挙動スタイル設定
///
/// 4 方向の余白は「用紙のどこを本文領域として使うか」という見た目の判断なので style が所有する
/// （用紙そのものの寸法は物理設定として `config.toml` の `[pdf]` が持つ）。余白単体の不正
/// （負値）はここの garde が、用紙寸法との組み合わせでしか判定できない制約は
/// [`crate::typeset::validate_layout`] が検証する。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PageStyle {
  /// 本文領域の上余白（ページ上端から本文上端まで）
  #[garde(custom(non_negative))]
  pub margin_top: Length,
  /// 本文領域の下余白（本文下端からページ下端まで）
  #[garde(custom(non_negative))]
  pub margin_bottom: Length,
  /// 本文領域の左余白（ページ左端から本文左端まで）
  #[garde(custom(non_negative))]
  pub margin_left: Length,
  /// 本文領域の右余白（本文右端からページ右端まで）
  #[garde(custom(non_negative))]
  pub margin_right: Length,
  /// 下端揃えを有効にするか
  pub flush_bottom: bool,
}

impl Default for PageStyle {
  fn default() -> Self {
    return Self {
      margin_top: Length::pt(99.0),
      margin_bottom: Length::pt(99.0),
      margin_left: Length::pt(85.0),
      margin_right: Length::pt(85.0),
      flush_bottom: false,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::PageStyle;
  use crate::length::Length;

  #[test]
  fn default_disables_flush_bottom() {
    let style = PageStyle::default();

    assert!(!style.flush_bottom);
  }

  #[test]
  fn default_matches_documented_margins() {
    let style = PageStyle::default();

    assert!((style.margin_top.to_pt() - 99.0).abs() < f32::EPSILON);
    assert!((style.margin_bottom.to_pt() - 99.0).abs() < f32::EPSILON);
    assert!((style.margin_left.to_pt() - 85.0).abs() < f32::EPSILON);
    assert!((style.margin_right.to_pt() - 85.0).abs() < f32::EPSILON);
  }

  #[test]
  fn validate_accepts_enabled_flush_bottom() {
    let style = PageStyle {
      flush_bottom: true,
      ..PageStyle::default()
    };

    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_accepts_zero_margins() {
    let style = PageStyle {
      margin_top: Length::ZERO,
      margin_bottom: Length::ZERO,
      margin_left: Length::ZERO,
      margin_right: Length::ZERO,
      ..PageStyle::default()
    };

    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_margin() {
    let style = PageStyle {
      margin_left: Length::pt(-1.0),
      ..PageStyle::default()
    };

    assert!(style.validate().is_err());
  }
}
