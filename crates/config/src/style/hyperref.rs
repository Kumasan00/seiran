//! ハイパーリンク（hyperref 相当）のスタイル設定型。

use garde::Validate;
use model::Color;
use serde::{Deserialize, Serialize};

/// ハイパーリンクの文字色に関するスタイル設定
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
}

impl Default for HyperrefStyle {
  fn default() -> Self {
    return Self {
      link_color: None,
      url_color: None,
      cite_color: None,
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
  fn rejects_renamed_show_bookmarks_key() {
    assert!(toml::from_str::<HyperrefStyle>("show_bookmarks = true\n").is_err());
  }

  #[test]
  fn default_colors_are_none() {
    let style = HyperrefStyle::default();
    assert_eq!(style.link_color, None);
    assert_eq!(style.url_color, None);
    assert_eq!(style.cite_color, None);
  }
}
