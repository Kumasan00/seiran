//! タイトルページ（`\maketitle` 相当）のスタイル設定型。
//!
//! 文書の先頭に自動生成のタイトルページを挿入する機能（LaTeX の `titlepage` 相当）の
//! 見た目と有効化を定義する。`[title_page]` テーブルにマップされ、[`crate::read_style::Style`]
//! の `title_page` フィールドに置かれる。
//!
//! 「コンテンツとプレゼンテーションの完全分離」「プリアンブル相当はソースに書けない」に従い、
//! タイトルページの **内容** は `config.toml` の `[document]`（title / author / date）から、
//! **見た目・有効化** はこのスタイルから供給する。ソース側にコマンド（`\maketitle` 等）は
//! 追加しない。タイトル・著者・日付は中央寄せの段落として縦に積まれる（揃えは中央固定）。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{
  FontKind,
  length::{Length, non_negative, positive},
};

/// タイトルページのスタイル設定。
///
/// `enabled` が `false`（既定）のときはタイトルページを生成しない（オプトイン）。
/// 各要素（タイトル / 著者 / 日付）のフォントサイズ・種別と要素間の縦アキ、ページ上端から
/// タイトルまでの送り（`top_margin`）を持つ。垂直方向は `top_margin` による簡易指定のみで、
/// 垂直中央寄せは将来対応（issue のスコープ外）。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct TitlePageStyle {
  /// タイトルページを生成するか（既定 `false` = 無効）
  #[garde(skip)]
  pub enabled: bool,
  /// ページ上端からタイトルまでの送り（垂直位置の簡易指定）
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// タイトルのフォントサイズ
  #[garde(custom(positive))]
  pub title_font_size: Length,
  /// タイトルのフォント種別
  #[garde(skip)]
  pub title_font_kind: FontKind,
  /// タイトルの下の縦アキ（タイトルと著者の間隔）
  #[garde(custom(non_negative))]
  pub title_bottom_margin: Length,
  /// 著者のフォントサイズ
  #[garde(custom(positive))]
  pub author_font_size: Length,
  /// 著者のフォント種別
  #[garde(skip)]
  pub author_font_kind: FontKind,
  /// 著者の下の縦アキ（著者と日付の間隔）
  #[garde(custom(non_negative))]
  pub author_bottom_margin: Length,
  /// 日付のフォントサイズ
  #[garde(custom(positive))]
  pub date_font_size: Length,
  /// 日付のフォント種別
  #[garde(skip)]
  pub date_font_kind: FontKind,
}

impl Default for TitlePageStyle {
  /// 既定値: 無効（`enabled = false`）。タイトル 36pt 太字、著者 18pt、日付 14pt。
  fn default() -> Self {
    return Self {
      enabled: false,
      top_margin: Length::pt(120.0),
      title_font_size: Length::pt(36.0),
      title_font_kind: FontKind::SerifBold,
      title_bottom_margin: Length::pt(24.0),
      author_font_size: Length::pt(18.0),
      author_font_kind: FontKind::Serif,
      author_bottom_margin: Length::pt(12.0),
      date_font_size: Length::pt(14.0),
      date_font_kind: FontKind::Serif,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{FontKind, length::Length};

  use super::TitlePageStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(TitlePageStyle::default().validate().is_ok());
  }

  #[test]
  fn default_is_disabled() {
    // Arrange / Act
    let style = TitlePageStyle::default();

    // Assert — 既定はオプトインなので無効
    assert!(!style.enabled);
    assert_eq!(style.title_font_kind, FontKind::SerifBold);
    assert!((style.title_font_size.to_pt() - 36.0).abs() < f32::EPSILON);
  }

  #[test]
  fn deserializes_partial_with_defaults() {
    // Arrange — enabled だけ指定し、残りは既定値でマージされる
    let toml = "enabled = true\n";

    // Act
    let style: TitlePageStyle = toml::from_str(toml).unwrap();

    // Assert
    assert!(style.enabled);
    assert!((style.author_font_size.to_pt() - 18.0).abs() < f32::EPSILON);
  }

  #[test]
  fn validate_rejects_zero_title_font_size() {
    // Arrange
    let style = TitlePageStyle {
      title_font_size: Length::pt(0.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    // Arrange
    let style = TitlePageStyle {
      top_margin: Length::pt(-1.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_bottom_margin() {
    // Arrange — 縦アキ 0 は許容（非負）
    let style = TitlePageStyle {
      title_bottom_margin: Length::pt(0.0),
      author_bottom_margin: Length::pt(0.0),
      ..TitlePageStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }
}
