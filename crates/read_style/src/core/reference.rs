//! 参考文献セクションのスタイル設定型。

use std::path::PathBuf;

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::length::{Length, non_negative, positive};

/// 参考文献セクションのスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub struct ReferenceStyle {
  /// 参考文献セクションのタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// セクションのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// セクションブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 引用整形に用いる CSL スタイルファイル（`.csl`）のパス。
  ///
  /// IEEE / APA などの整形規則（採番方式・書誌の体裁）を定める独立 CSL スタイル
  /// （independent style）を指す。引用の見た目を決める設定なので style.toml に置く。
  /// `None`（既定）で引用（`\cite`）が存在する場合は `citation` がエラーを報告する。
  /// パスの読み込み・解析は `locale_path` と同様に `citation` クレートが [`crate::read_style`]
  /// 後に実施する（`read_style` では検証・正規化しない）。
  #[garde(skip)]
  pub csl_path: Option<PathBuf>,
  /// 引用整形に用いる CSL ロケールファイル（`.xml`）のパス。
  ///
  /// hayagriva 内蔵ロケールを上書き・補強するために使う。CSL ロケール XML（`locales-xx-YY.xml`
  /// 形式）を指す。CSL は書誌全体を 1 つの有効ロケール（`.csl` の default-locale）で整形するため、
  /// 指定するロケールはその有効言語コードに一致させる。一致すれば内蔵ロケールより優先される。
  /// `None`（既定）の場合は内蔵ロケールのみを使う。
  /// パスの読み込み・解析は `citation` クレートが [`crate::read_style`] 後に実施する。
  #[garde(skip)]
  pub locale_path: Option<PathBuf>,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      title: "References".to_string(),
      font_size: Length::pt(12.0),
      bottom_margin: Length::pt(10.0),
      csl_path: None,
      locale_path: None,
    };
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use garde::Validate;

  use super::ReferenceStyle;
  use crate::parse_style;

  #[test]
  fn validate_accepts_default() {
    assert!(ReferenceStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    let style = ReferenceStyle {
      title: String::new(),
      ..ReferenceStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn default_locale_path_is_none() {
    assert!(ReferenceStyle::default().locale_path.is_none());
  }

  #[test]
  fn parse_style_accepts_top_level_reference_table() {
    // Arrange: トップレベルの [reference] テーブル（csl_path / locale_path を含む）
    let toml = "[reference]\n\
                title = \"参考文献\"\n\
                csl_path = \"styles/ieee.csl\"\n\
                locale_path = \"locales/locales-ja-JP.xml\"\n";

    // Act
    let style = parse_style(toml, "style.toml").expect("[reference] を含む style.toml はパースできるはず");

    // Assert
    assert_eq!(style.core.reference.title, "参考文献");
    assert_eq!(style.core.reference.csl_path, Some(PathBuf::from("styles/ieee.csl")));
    assert_eq!(style.core.reference.locale_path, Some(PathBuf::from("locales/locales-ja-JP.xml")));
  }
}
