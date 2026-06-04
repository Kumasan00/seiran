//! 数式レイアウト（上付き・下付きスクリプトのサイズと位置）のスタイル設定型。
//!
//! `parser::document::MathStyle`（数式の Bold/Italic 等の意味的スタイル）と
//! 役割が異なるため、混同を避けるべく `MathLayoutStyle` という名称にしている。
//! `crates/layout/src/lowering/math.rs` で参照される定数群（`SCRIPT_SIZE_FACTOR`、
//! `SUPERSCRIPT_RAISE_FACTOR`、`SUBSCRIPT_DROP_FACTOR`、最小スクリプトフォントサイズ）を
//! 設定可能化する。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// 数式レイアウトのスタイル設定
#[derive(Debug, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields)]
pub struct MathLayoutStyle {
  /// 上付き / 下付きスクリプトのフォントサイズ倍率（親フォントサイズに対する比）。
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub script_size_factor: f32,
  /// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）。
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub superscript_raise_factor: f32,
  /// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）。
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub subscript_drop_factor: f32,
  /// スクリプトフォントサイズの下限（pt）。極端な縮小を防ぐためのクランプ値。
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub min_script_font_size: f32,
}

impl Default for MathLayoutStyle {
  fn default() -> Self {
    // 既定値は `crates/layout/src/lowering/math.rs` の現行ハードコード定数と一致させる
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

  use super::MathLayoutStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(MathLayoutStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_current_hardcoded_values() {
    // Arrange / Act
    let style = MathLayoutStyle::default();

    // Assert: layout 側の現行 const と一致していること
    assert!((style.script_size_factor - 0.7).abs() < f32::EPSILON);
    assert!((style.superscript_raise_factor - 0.4).abs() < f32::EPSILON);
    assert!((style.subscript_drop_factor - 0.2).abs() < f32::EPSILON);
    assert!((style.min_script_font_size - 6.0).abs() < f32::EPSILON);
  }

  #[test]
  fn validate_rejects_zero_script_size_factor() {
    // Arrange: range(min = MIN_POSITIVE) のため 0.0 は不正
    let style = MathLayoutStyle {
      script_size_factor: 0.0,
      ..MathLayoutStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_superscript_raise_factor() {
    // Arrange
    let style = MathLayoutStyle {
      superscript_raise_factor: -0.1,
      ..MathLayoutStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_subscript_drop_factor() {
    // Arrange
    let style = MathLayoutStyle {
      subscript_drop_factor: -0.1,
      ..MathLayoutStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_raise_factor() {
    // Arrange: 上付きシフトを 0 にする（同じベースラインに描く）のは設定として有効
    let style = MathLayoutStyle {
      superscript_raise_factor: 0.0,
      ..MathLayoutStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_min_script_font_size() {
    // Arrange: range(min = MIN_POSITIVE) のため 0.0 は不正
    let style = MathLayoutStyle {
      min_script_font_size: 0.0,
      ..MathLayoutStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
