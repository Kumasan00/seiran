//! 参考文献セクションのスタイル設定型。

use std::path::PathBuf;

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::model::{
  Length,
  length::{non_negative, positive},
};

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
  /// `None`（既定）で引用（`\cite`）が存在する場合は `citation` がエラーを報告する。
  /// [`read_style`](crate::config::style::read_style) が絶対パスへ正規化する。
  #[garde(skip)]
  pub csl_path: Option<PathBuf>,
  /// 引用整形に用いる CSL ロケールファイル（`.xml`）のパス。
  ///
  /// 同一言語コードでは内蔵ロケールより優先される。`None` の場合は内蔵ロケールのみを使う。
  #[garde(skip)]
  pub locale_path: Option<PathBuf>,
  /// 書誌の出力言語（CSL の active locale）を選ぶロケールコード（例 `"ja-JP"`）。
  ///
  /// active locale は次の優先順位で決まる: 本フィールド → [`locale_path`](Self::locale_path) の
  /// ファイルの `xml:lang` → `.csl` の `default-locale`（最終的に en-US）。
  /// [`read_style`](crate::config::style::read_style) が BCP 47 の標準形へ正規化する。
  #[garde(custom(validate_locale))]
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

impl ReferenceStyle {
  /// 値を正規化する（現状はロケールコードを BCP 47 の標準形へ揃える）。
  pub fn normalize(&mut self) {
    if let Some(code) = &self.locale
      && let Ok(langid) = unic_langid::LanguageIdentifier::from_bytes(code.as_bytes())
    {
      self.locale = Some(langid.to_string());
    }
  }
}

/// ロケールコードの構文を検証する `garde` カスタムバリデーター。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_locale(value: &Option<String>, _: &()) -> garde::Result {
  let Some(code) = value else {
    return Ok(());
  };
  unic_langid::LanguageIdentifier::from_bytes(code.as_bytes()).map_err(|e| {
    return garde::Error::new(format!("ロケールコードが BCP 47 として不正です: {e}（受け取った値: {code:?}）"));
  })?;
  return Ok(());
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use garde::Validate;

  use super::ReferenceStyle;
  use crate::config::style::parse_style;

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
    // Arrange
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

  #[test]
  fn validate_accepts_well_formed_locale() {
    // Arrange / Act / Assert
    for code in ["en", "ja-JP", "zh-Hant", "es-419"] {
      let style = ReferenceStyle {
        locale: Some(code.to_string()),
        ..ReferenceStyle::default()
      };
      assert!(style.validate().is_ok(), "{code:?} は妥当なロケールとして許可されるべき");
    }
  }

  #[test]
  fn validate_accepts_locale_regardless_of_case() {
    let style = ReferenceStyle {
      locale: Some("ja-jp".to_string()),
      ..ReferenceStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_malformed_locale() {
    // Arrange / Act / Assert
    for bad in ["", "j", "ja-", "ja JP"] {
      let style = ReferenceStyle {
        locale: Some(bad.to_string()),
        ..ReferenceStyle::default()
      };
      assert!(style.validate().is_err(), "{bad:?} は不正なロケールとして拒否されるべき");
    }
  }

  #[test]
  fn normalize_canonicalizes_locale_case() {
    // Arrange
    let mut style = ReferenceStyle {
      locale: Some("EN-us".to_string()),
      ..ReferenceStyle::default()
    };

    // Act
    style.normalize();

    // Assert
    assert_eq!(style.locale.as_deref(), Some("en-US"));
  }

  #[test]
  fn normalize_canonicalizes_script_subtag() {
    // Arrange
    let mut style = ReferenceStyle {
      locale: Some("zh-HANT".to_string()),
      ..ReferenceStyle::default()
    };

    // Act
    style.normalize();

    // Assert
    assert_eq!(style.locale.as_deref(), Some("zh-Hant"));
  }

  #[test]
  fn normalize_canonicalizes_underscore_separator() {
    // Arrange
    let mut style = ReferenceStyle {
      locale: Some("ja_JP".to_string()),
      ..ReferenceStyle::default()
    };

    // Act
    style.normalize();

    // Assert
    assert_eq!(style.locale.as_deref(), Some("ja-JP"));
  }

  #[test]
  fn normalize_keeps_none_locale() {
    // Arrange / Act / Assert
    let mut style = ReferenceStyle::default();
    style.normalize();
    assert!(style.locale.is_none());
  }
}
