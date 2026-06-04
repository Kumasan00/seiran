//! 数式レイアウト（上付き / 下付きスクリプトのサイズと位置）のスタイル設定型。
//!
//! `parser::document::MathStyle`（数式の Bold/Italic などの意味的スタイル）と名前が衝突しないよう、
//! 利用側では `read_style::MathStyle` で qualify するか `as MathStyleConfig` 等の別名を使うこと。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 数式レイアウトのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct MathStyle {
  /// 上付き / 下付きスクリプトのフォントサイズ倍率（親フォントサイズに対する比）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub script_size_factor: f32,
  /// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub superscript_raise_factor: f32,
  /// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub subscript_drop_factor: f32,
  /// スクリプトフォントサイズの下限（pt）。極端な縮小を防ぐためのクランプ値
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub min_script_font_size: f32,
}

impl Default for MathStyle {
  fn default() -> Self {
    return Self {
      script_size_factor: 0.7,
      superscript_raise_factor: 0.4,
      subscript_drop_factor: 0.2,
      min_script_font_size: 6.0,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::MathStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(MathStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_script_size_factor() {
    // Arrange
    let style = MathStyle {
      script_size_factor: 0.0,
      ..MathStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_raise_factor() {
    // Arrange
    let style = MathStyle {
      superscript_raise_factor: -0.1,
      ..MathStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_raise_factor() {
    // Arrange: 上付きシフトを 0 にする（同じベースライン）も有効
    let style = MathStyle {
      superscript_raise_factor: 0.0,
      ..MathStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
