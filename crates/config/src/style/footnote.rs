//! 脚注（`\footnote`）のスタイル設定型（`[footnote]` テーブル）。
//!
//! 本文中のマーカー（上付き番号）と脚注本体先頭のマーカーは同じ書式・縮小率・上付きシフト量を
//! 共有する（`typeset::lowering` が基準フォントサイズだけを使い分けて適用する）。区切り罫線は
//! `top_margin` → 罫線（`rule_length` × `rule_thickness`） → `rule_gap` → 脚注本体、の順に積む。

use garde::Validate;
use model::{
  Color,
  length::{Length, non_negative, positive},
};
use serde::{Deserialize, Serialize};

/// 脚注のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct FootnoteStyle {
  /// 脚注本体のフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// マーカー番号の書式テンプレート（`{number}` を置換。本文中・脚注本体先頭の両方に使う）
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::tag_format))]
  pub marker_format: String,
  /// マーカーの縮小率（基準フォントサイズに対する比）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub marker_size_factor: f32,
  /// マーカーの上付きシフト量（基準フォントサイズに対する比、正で上方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub marker_raise_factor: f32,
  /// 本文と区切り罫線の間隔
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 区切り罫線の長さ
  #[garde(custom(non_negative))]
  pub rule_length: Length,
  /// 区切り罫線の太さ（0 のとき描画しない）
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// 区切り罫線の色。`None` は黒
  pub rule_color: Option<Color>,
  /// 区切り罫線〜最初の脚注、および脚注どうしの間隔
  #[garde(custom(non_negative))]
  pub rule_gap: Length,
}

impl Default for FootnoteStyle {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(9.0),
      marker_format: "{number}".to_string(),
      marker_size_factor: 0.7,
      marker_raise_factor: 0.4,
      top_margin: Length::pt(6.0),
      rule_length: Length::pt(72.0),
      rule_thickness: Length::pt(0.4),
      rule_color: None,
      rule_gap: Length::pt(4.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::Length;

  use super::FootnoteStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(FootnoteStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = FootnoteStyle {
      font_size: Length::pt(0.0),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = FootnoteStyle {
      rule_thickness: Length::pt(-0.1),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    let style = FootnoteStyle {
      top_margin: Length::pt(-0.1),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_marker_raise_factor() {
    // 上付きシフトを 0 にする（同じベースライン）も有効
    let style = FootnoteStyle {
      marker_raise_factor: 0.0,
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_marker_size_factor() {
    let style = FootnoteStyle {
      marker_size_factor: 0.0,
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_unknown_marker_format_token() {
    let style = FootnoteStyle {
      marker_format: "{title}".to_string(),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_decorated_marker_format() {
    let style = FootnoteStyle {
      marker_format: "[{number}]".to_string(),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_ok());
  }
}
