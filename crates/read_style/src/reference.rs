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
  /// パスは [`read_style`](crate::read_style) が `canonicalize` で絶対パスへ正規化し、同時に
  /// ファイルの存在を検証する（解決できなければ [`crate::ValidationError::CslPathResolution`]）。
  /// ファイル内容の読み込み・解析（CSL スタイルのパース）は `citation` クレートが後段で行う。
  #[garde(skip)]
  pub csl_path: Option<PathBuf>,
  /// 引用整形に用いる CSL ロケールファイル（`.xml`）のパス。
  ///
  /// hayagriva 内蔵ロケールを補強・上書きするために使う。CSL ロケール XML（`locales-xx-YY.xml`
  /// 形式）を指す。`citation` はこのファイルを内蔵ロケール一式の**前**に重ねる（overlay）ため、
  /// 同一言語コードのロケールは内蔵より優先される。`None`（既定）の場合は内蔵ロケールのみを使う。
  /// なお書誌の出力言語（active locale）は [`locale`](Self::locale) で決まり、`locale` が未指定の
  /// ときに限り、本ファイルの `xml:lang` がその既定値として採用される。
  /// パスは [`read_style`](crate::read_style) が `canonicalize` で絶対パスへ正規化し、ファイルの
  /// 存在を検証する（解決できなければ [`crate::ValidationError::LocalePathResolution`]）。
  /// ファイル内容の読み込み・解析（ロケール XML のパース）は `citation` クレートが後段で行う。
  #[garde(skip)]
  pub locale_path: Option<PathBuf>,
  /// 書誌の出力言語（CSL の active locale）を選ぶロケールコード（例 `"ja-JP"`）。
  ///
  /// ja-JP・de-DE などの用語セットは hayagriva に内蔵されているため、言語を選ぶだけなら
  /// [`locale_path`](Self::locale_path) のファイルは不要で、本フィールドの指定だけで足りる。
  /// active locale は次の優先順位で決まる: 本フィールド → [`locale_path`](Self::locale_path) の
  /// ファイルの `xml:lang` → `.csl` の `default-locale`（最終的に en-US）。`citation` クレートが
  /// `BibliographyRequest` / `CitationRequest` の locale override として解釈する。
  /// [`read_style`](crate::read_style) がロケールコードの構文を検証し（不正なら
  /// [`crate::ValidationError::Field`]）、大文字小文字を BCP 47 の標準形へ正規化する
  /// （例 `ja-jp` → `ja-JP`。言語=小文字・地域=大文字・用字=先頭大文字）。
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
  ///
  /// [`crate::parse_style`] が値検証の後に呼び出す純粋処理。`locale` が `Some` のとき、
  /// [`unic_langid::LanguageIdentifier`] でパースしてから `to_string` で標準形へ整える（言語サブ
  /// タグを小文字・地域サブタグを大文字・用字サブタグを先頭大文字へ、区切りは `-` へ揃える。
  /// 例 `ja-jp` → `ja-JP`、`ja_JP` → `ja-JP`）。`citation` の内蔵ロケール照合は文字列一致のため、
  /// ここで標準形へ揃えておくと照合が安定する。`None` のときは何もしない。検証済みの値はパースに
  /// 成功する前提だが、万一失敗した場合は元の値を温存する。パスの正規化（`canonicalize`）は I/O を
  /// 伴うため [`crate::read_style`] 側で行う。
  pub fn normalize(&mut self) {
    if let Some(code) = &self.locale
      && let Ok(langid) = unic_langid::LanguageIdentifier::from_bytes(code.as_bytes())
    {
      self.locale = Some(langid.to_string());
    }
  }
}

/// ロケールコードの構文を検証する `garde` カスタムバリデーター。
///
/// `None`（ロケール未指定）は許可する。`Some` のときは [`unic_langid::LanguageIdentifier::from_bytes`]
/// による BCP 47 パースが成功するかだけを確認し、大文字小文字は問わない（[`ReferenceStyle::normalize`]
/// が後段で標準形へ揃える）。`unic-langid` は `-`/`_` の双方を区切りとして受理する。実在するロケール
/// かどうかは検証しない（内蔵 / カスタムロケールの有無は `citation` が解決する）。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_locale(value: &Option<String>, _: &()) -> garde::Result {
  let Some(code) = value else {
    return Ok(());
  };
  unic_langid::LanguageIdentifier::from_bytes(code.as_bytes())
    .map_err(|e| garde::Error::new(format!("ロケールコードが BCP 47 として不正です: {e}（受け取った値: {code:?}）")))?;
  return Ok(());
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

  #[test]
  fn validate_accepts_well_formed_locale() {
    // Arrange / Act / Assert — 言語のみ・言語-地域・言語-用字 はいずれも許可
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
    // 大文字小文字は問わない（正規化が後段で標準形へ揃える）
    let style = ReferenceStyle {
      locale: Some("ja-jp".to_string()),
      ..ReferenceStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_malformed_locale() {
    // Arrange / Act / Assert — 空・短すぎる言語・末尾の空サブタグ・空白
    // （`ja_JP` のアンダースコア区切り・`english` の長い言語サブタグは unic-langid が受理する）
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

    // Assert — 言語=小文字・地域=大文字へ揃う
    assert_eq!(style.locale.as_deref(), Some("en-US"));
  }

  #[test]
  fn normalize_canonicalizes_script_subtag() {
    // Arrange — 用字サブタグ（4 文字）は先頭大文字 + 残り小文字
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
    // Arrange — unic-langid はアンダースコア区切りを受理し、正規化でハイフン区切りへ正準化する
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
    // Arrange / Act / Assert — locale が None なら何もしない
    let mut style = ReferenceStyle::default();
    style.normalize();
    assert!(style.locale.is_none());
  }
}
