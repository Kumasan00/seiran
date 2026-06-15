//! ハイパーリンク（hyperref 相当）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::Color;

/// ハイパーリンクとしおりに関するスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct HyperrefStyle {
  /// 内部参照リンク（`\ref` 等）の文字色。`None` は本文色を継承
  pub link_color: Option<Color>,
  /// 外部 URL リンクの文字色。`None` は本文色を継承
  pub url_color: Option<Color>,
  /// 文献引用（`\cite`）の文字色。`None` は本文色を継承
  pub cite_color: Option<Color>,
  /// PDF のしおり（ブックマーク）を出力するか
  pub show_bookmarks: bool,
}

impl Default for HyperrefStyle {
  fn default() -> Self {
    return Self {
      link_color: Some(Color::new(0, 0, 255)),
      url_color: Some(Color::new(0, 0, 255)),
      cite_color: Some(Color::new(0, 0, 255)),
      show_bookmarks: true,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::HyperrefStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(HyperrefStyle::default().validate().is_ok());
  }

  #[test]
  fn default_enables_bookmarks() {
    assert!(HyperrefStyle::default().show_bookmarks);
  }
}
