//! 拡張スタイル設定（[`ExtendedStyle`]）。
//!
//! 脚注といった、現状は `lowering` / `pdf_gen` 側で
//! 参照されていない（実装が追いついていない）スタイル設定を
//! [`crate::Style`] の `extended` フィールドに分離して保持する。
//!
//! 利用側からは `style.extended.footnote` のように経由してアクセスする。
//! TOML では `[extended.footnote]` / … の各テーブルにマップされる。

pub mod footnote;

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::extended::footnote::FootnoteStyle;

/// 拡張スタイル設定。資産として保持しているが、現状は `lowering/pdf_gen` から参照されていない。
#[derive(Debug, Clone, Default, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct ExtendedStyle {
  /// 脚注のスタイル
  #[garde(dive)]
  pub footnote: FootnoteStyle,
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::ExtendedStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(ExtendedStyle::default().validate().is_ok());
  }

  #[test]
  fn default_round_trips_through_toml() {
    // Arrange / Act: serialize → deserialize して同等の構造が復元できる
    let style = ExtendedStyle::default();
    let toml = toml::to_string(&style).unwrap();
    let restored: ExtendedStyle = toml::from_str(&toml).unwrap();

    // Assert
    assert!(restored.validate().is_ok());
    assert_eq!(restored.footnote.font_size, style.footnote.font_size);
  }
}
