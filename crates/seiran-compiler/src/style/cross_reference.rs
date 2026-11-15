//! 相互参照（`\refs` / `\refrange`）の区切りのスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::project::non_empty_text;

/// 複数参照・範囲参照の番号の区切り（`[cross_reference]`）
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct CrossReferenceStyle {
  /// `\refs` の番号どうしの区切り（例: `", "`、`"、"`）
  #[garde(custom(non_empty_text))]
  pub list_separator: String,
  /// `\refrange` の始点と終点の区切り（例: `"–"`、`"〜"`）
  #[garde(custom(non_empty_text))]
  pub range_separator: String,
}

impl Default for CrossReferenceStyle {
  fn default() -> Self {
    return Self {
      list_separator: ", ".to_string(),
      range_separator: "\u{2013}".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::CrossReferenceStyle;
  use crate::style::Style;

  #[test]
  fn default_uses_comma_and_en_dash() {
    let style = CrossReferenceStyle::default();

    assert_eq!(style.list_separator, ", ");
    assert_eq!(style.range_separator, "\u{2013}");
  }

  #[test]
  fn partial_override_keeps_other_default() {
    let style: Style = toml::from_str("[cross_reference]\nlist_separator = \"、\"\n").unwrap();

    assert_eq!(style.cross_reference.list_separator, "、");
    assert_eq!(style.cross_reference.range_separator, "\u{2013}");
  }

  #[test]
  fn validate_rejects_empty_separator() {
    for toml in [
      "[cross_reference]\nlist_separator = \"\"\n",
      "[cross_reference]\nrange_separator = \"\"\n",
    ] {
      let style: Style = toml::from_str(toml).unwrap();

      assert!(style.validate().is_err(), "{toml}");
    }
  }

  #[test]
  fn rejects_unknown_key() {
    assert!(toml::from_str::<Style>("[cross_reference]\nfoo = 1\n").is_err());
  }
}
