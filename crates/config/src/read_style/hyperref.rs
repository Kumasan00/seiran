//! ハイパーリンク（hyperref 相当）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::Color;

/// ハイパーリンクの文字色に関するスタイル設定
///
/// しおり（ブックマーク）の出力可否は「文字の見た目」ではなく PDF の構造的な出力機能のため、
/// `config.toml` の `[pdf].show_bookmarks` が担う（#127 で style から config へ移動）。
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
      // 既定はリンク色を付けず、本文色（黒）を継承する（`None`）。色を付けたい場合は
      // style.toml の `[hyperref]` で `link_color` 等を明示する。
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
    // しおり出力は #127 で config `[pdf]` へ移動した。style 側旧キーは未知フィールドとして弾く。
    assert!(toml::from_str::<HyperrefStyle>("show_bookmarks = true\n").is_err());
  }

  #[test]
  fn default_colors_are_none() {
    // 既定はリンク色を付けず本文色（黒）を継承する（`None`）
    let style = HyperrefStyle::default();
    assert_eq!(style.link_color, None);
    assert_eq!(style.url_color, None);
    assert_eq!(style.cite_color, None);
  }
}
