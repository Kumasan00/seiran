//! 目次（table of contents）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, non_negative, positive};

/// 目次のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TocStyle {
  /// 目次のタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// 目次に含める見出しの最大深さ。1=part のみ、`HeadingLevel::COUNT`=subparagraph まで
  ///
  /// `types::HeadingLevel` の数と整合させるため、上限を 6 で固定する。
  #[garde(range(min = 1, max = 6))]
  pub max_depth: u32,
  /// 目次エントリのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 目次ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// ページ番号を表示するか
  pub show_page_numbers: bool,
}

impl Default for TocStyle {
  fn default() -> Self {
    return Self {
      title: "Contents".to_string(),
      max_depth: 3,
      font_size: Length::pt(12.0),
      bottom_margin: Length::pt(10.0),
      show_page_numbers: true,
    };
  }
}

/// 型レベルの整合チェック: `TocStyle::max_depth` の上限が `HeadingLevel::COUNT` と一致する
///
/// `garde` の `range` 属性は const 式しか受け付けないため上限値はリテラルだが、ここで
/// 静的アサートを置くことで `HeadingLevel` を増減した際に誤値を検出できる。
const _: () = assert!(types::HeadingLevel::COUNT == 6);

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TocStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(TocStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_title() {
    let style = TocStyle {
      title: String::new(),
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_max_depth() {
    let style = TocStyle {
      max_depth: 0,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_too_large_max_depth() {
    let style = TocStyle {
      max_depth: 7,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
