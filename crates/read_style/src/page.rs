//! ページ組版の挙動スタイル設定型。
//!
//! 「同じ本文 + 同じ用紙で見た目だけ差し替える」対象のうち、段組み（[`crate::columns`]）とは別の
//! ページ単位の組版挙動を集約する。用紙サイズ・余白は物理設定として `config.toml` 側に残り、本型は
//! `style.toml`（`[page]`）に置く。実際の組版判断は `hlist::break_pages` が担い、本型はそのフラグだけを保持する。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// ページ組版の挙動スタイル設定
///
/// `flush_bottom` は下端揃え（各ページ・各段の最終ベースラインを版面下端へ揃える）の有効化スイッチ。
/// 既定 `false`（従来どおりの ragged bottom）。有効時は満杯ページの不足高さを段落間・見出し前後等の
/// 伸縮アキへ配分する（`hlist::break_pages`）。最終ページ・強制改ページ直前・伸縮アキの無いページは
/// 揃えない（自然高のまま）。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct PageStyle {
  /// 下端揃え（flush bottom）を有効にするか（既定 `false`）
  pub flush_bottom: bool,
}

impl Default for PageStyle {
  fn default() -> Self {
    return Self {
      flush_bottom: false,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::PageStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(PageStyle::default().validate().is_ok());
  }

  #[test]
  fn default_disables_flush_bottom() {
    // Arrange / Act
    let style = PageStyle::default();

    // Assert
    assert!(!style.flush_bottom);
  }

  #[test]
  fn validate_accepts_enabled_flush_bottom() {
    // Arrange
    let style = PageStyle { flush_bottom: true };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
