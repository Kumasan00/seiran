//! ハイパーリンク（hyperref 相当）のスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::color::Color;

/// ハイパーリンクの文字色に関するスタイル設定
#[expect(
  clippy::struct_field_names,
  reason = "3 フィールドとも `style.toml` の TOML キーに直接対応し、`_color` を外すのはスキーマの破壊的変更になる"
)]
#[derive(Debug, Clone, Default, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct HyperrefStyle {
  /// 内部参照リンク（`\ref` 等）の文字色。`None` は本文色を継承
  pub link_color: Option<Color>,
  /// 外部 URL リンクの文字色。`None` は本文色を継承
  pub url_color: Option<Color>,
  /// 文献引用（`\cite`）の文字色。`None` は本文色を継承
  pub cite_color: Option<Color>,
}

#[cfg(test)]
mod tests {
  use super::HyperrefStyle;

  #[test]
  fn rejects_renamed_show_bookmarks_key() {
    assert!(toml::from_str::<HyperrefStyle>("show_bookmarks = true\n").is_err());
  }
}
