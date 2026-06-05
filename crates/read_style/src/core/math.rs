//! 数式レイアウト（上付き / 下付きスクリプトのサイズと位置）のスタイル設定型。
//!
//! `parser::document::MathStyle`（Bold/Italic などの意味的スタイル）との名前衝突を避けるため、
//! および将来 OpenType MATH テーブルから自動取得する範囲を明確化するため、`MathScriptStyle`
//! と命名している。MATH テーブル対応時にはこれを `Option<MathScriptStyle>`（`None` = MATH
//! テーブルから自動取得）に変える想定で、現状の手動設定は暫定的な API 境界。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::primitives::length::{Length, positive};

/// 数式レイアウトのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct MathScriptStyle {
  /// 上付き / 下付きスクリプトのフォントサイズ倍率（親フォントサイズに対する比）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub script_size_factor: f32,
  /// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub superscript_raise_factor: f32,
  /// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub subscript_drop_factor: f32,
  /// スクリプトフォントサイズの下限。極端な縮小を防ぐためのクランプ値
  #[garde(custom(positive))]
  pub min_script_font_size: Length,
}

impl Default for MathScriptStyle {
  fn default() -> Self {
    return Self {
      script_size_factor: 0.7,
      superscript_raise_factor: 0.4,
      subscript_drop_factor: 0.2,
      min_script_font_size: Length::pt(6.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::MathScriptStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(MathScriptStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_script_size_factor() {
    // Arrange
    let style = MathScriptStyle {
      script_size_factor: 0.0,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_raise_factor() {
    // Arrange
    let style = MathScriptStyle {
      superscript_raise_factor: -0.1,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_raise_factor() {
    // Arrange: 上付きシフトを 0 にする（同じベースライン）も有効
    let style = MathScriptStyle {
      superscript_raise_factor: 0.0,
      ..MathScriptStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
