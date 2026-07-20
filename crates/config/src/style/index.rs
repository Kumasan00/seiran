//! 巻末索引のスタイル設定型。
//!
//! `[reference]` と同じく `enabled` を持たない — ソースに `\index` マーカーが 1 個以上あるときだけ
//! 自動出力される（issue #247）。段組み数は本文用 [`crate::style::ColumnsStyle`]（issue #32、1〜2 段
//! の contract）とは独立に持つ（索引は本文より多段にすることが一般的なため）。段間は本文と共通の
//! [`crate::style::ColumnsStyle::gap`] を流用し、専用フィールドは持たない。

use garde::Validate;
use model::{
  Length,
  length::{non_negative, positive},
};
use serde::{Deserialize, Serialize};

/// 巻末索引のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct IndexStyle {
  /// 索引ページのタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// タイトルのフォントサイズ
  #[garde(custom(positive))]
  pub title_font_size: Length,
  /// タイトルとエントリ列の間の下余白
  #[garde(custom(non_negative))]
  pub title_bottom_margin: Length,
  /// エントリ（語 + ページ番号列）のフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 索引ページの段組み数（1 = 単段）
  #[garde(range(min = 1, max = 3))]
  pub column_count: u8,
  /// 語とページ番号列の間の水平アキ
  #[garde(custom(non_negative))]
  pub entry_gap: Length,
  /// 索引ブロック全体の下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for IndexStyle {
  fn default() -> Self {
    return Self {
      title: "Index".to_string(),
      title_font_size: Length::pt(18.0),
      title_bottom_margin: Length::pt(12.0),
      font_size: Length::pt(10.0),
      column_count: 2,
      entry_gap: Length::pt(6.0),
      bottom_margin: Length::pt(10.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::Length;

  use super::IndexStyle;

  #[test]
  fn validate_accepts_default() {
    assert!(IndexStyle::default().validate().is_ok());
  }

  #[test]
  fn default_has_no_enabled_flag_and_two_columns() {
    // 索引はマーカー駆動の自動出力（enabled フラグを持たない）。既定は 2 段組み
    let style = IndexStyle::default();
    assert_eq!(style.title, "Index");
    assert_eq!(style.column_count, 2);
  }

  #[test]
  fn partial_toml_keeps_other_defaults() {
    // Arrange / Act: title だけ指定しても他フィールドは既定で埋まる
    let style: IndexStyle = toml::from_str("title = \"索引\"\n").unwrap();

    // Assert
    assert_eq!(style.title, "索引");
    assert_eq!(style.column_count, 2);
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_title() {
    let style = IndexStyle {
      title: String::new(),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_columns() {
    let style = IndexStyle {
      column_count: 0,
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_too_many_columns() {
    let style = IndexStyle {
      column_count: 4,
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_entry_gap() {
    let style = IndexStyle {
      entry_gap: Length::pt(-1.0),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
