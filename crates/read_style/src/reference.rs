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
  /// hayagriva 内蔵ロケールを補強・上書きするために使う。CSL ロケール XML（`locales-xx-YY.xml`
  /// 形式）を指す。`citation` はこのファイルを内蔵ロケール一式の**前**に重ねる（overlay）ため、
  /// 同一言語コードのロケールは内蔵より優先される。`None`（既定）の場合は内蔵ロケールのみを使う。
  /// なお書誌の出力言語（active locale）は [`locale`](Self::locale) で決まり、`locale` が未指定の
  /// ときに限り、本ファイルの `xml:lang` がその既定値として採用される。
  /// パスの読み込み・解析は `citation` クレートが [`crate::read_style`] 後に実施する。
  #[garde(skip)]
  pub locale_path: Option<PathBuf>,
  /// 書誌の出力言語（CSL の active locale）を選ぶロケールコード（例 `"ja-JP"`）。
  ///
  /// ja-JP・de-DE などの用語セットは hayagriva に内蔵されているため、言語を選ぶだけなら
  /// [`locale_path`](Self::locale_path) のファイルは不要で、本フィールドの指定だけで足りる。
  /// active locale は次の優先順位で決まる: 本フィールド → [`locale_path`](Self::locale_path) の
  /// ファイルの `xml:lang` → `.csl` の `default-locale`（最終的に en-US）。`citation` クレートが
  /// `BibliographyRequest` / `CitationRequest` の locale override として解釈する
  /// （`read_style` では検証・正規化しない）。
  #[garde(skip)]
  pub locale: Option<String>,
}

impl Default for ReferenceStyle {
  fn default() -> Self {
    return Self {
      title: "References".to_string(),
      font_size: Length::pt(12.0),
      bottom_margin: Length::pt(10.0),
      csl_path: None,
      locale_path: None,
      locale: None,
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
  fn default_locale_is_none() {
    assert!(ReferenceStyle::default().locale.is_none());
  }

  #[test]
  fn parse_style_accepts_top_level_reference_table() {
    // Arrange: トップレベルの [reference] テーブル（csl_path / locale_path / locale を含む）
    let toml = "[reference]\n\
                title = \"参考文献\"\n\
                csl_path = \"styles/ieee.csl\"\n\
                locale_path = \"locales/locales-ja-JP.xml\"\n\
                locale = \"ja-JP\"\n";

    // Act
    let style = parse_style(toml, "style.toml").expect("[reference] を含む style.toml はパースできるはず");

    // Assert
    assert_eq!(style.reference.title, "参考文献");
    assert_eq!(style.reference.csl_path, Some(PathBuf::from("styles/ieee.csl")));
    assert_eq!(style.reference.locale_path, Some(PathBuf::from("locales/locales-ja-JP.xml")));
    assert_eq!(style.reference.locale.as_deref(), Some("ja-JP"));
  }
}
