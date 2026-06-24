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
  /// 値を正規化する（現状はロケールコードの大文字小文字を BCP 47 の標準形へ揃える）。
  ///
  /// [`crate::parse_style`] が値検証の後に呼び出す純粋処理。`locale` が `Some` のとき、言語サブ
  /// タグを小文字・地域サブタグを大文字・用字サブタグを先頭大文字へ整える（例 `ja-jp` → `ja-JP`）。
  /// `citation` の内蔵ロケール照合は文字列一致のため、ここで標準形へ揃えておくと照合が安定する。
  /// `None` のときは何もしない。パスの正規化（`canonicalize`）は I/O を伴うため [`crate::read_style`]
  /// 側で行う。
  pub fn normalize(&mut self) {
    if let Some(code) = &self.locale {
      self.locale = Some(normalize_locale_code(code));
    }
  }
}

/// ロケールコードの構文を検証する `garde` カスタムバリデーター。
///
/// `None`（ロケール未指定）は許可する。`Some` のときは BCP 47 風の構文（言語サブタグ + 任意の
/// サブタグ）に合致するかだけを確認し、大文字小文字は問わない（[`normalize_locale_code`] が後段で
/// 標準形へ揃える）。実在するロケールかどうかは検証しない（内蔵 / カスタムロケールの有無は
/// `citation` が解決する）。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_locale(value: &Option<String>, _: &()) -> garde::Result {
  let Some(code) = value else {
    return Ok(());
  };
  if !is_well_formed_locale(code) {
    return Err(garde::Error::new(format!(
      "ロケールコードの構文が不正です（例: \"ja-JP\"・\"en\"）（受け取った値: {code:?}）"
    )));
  }
  return Ok(());
}

/// ロケールコードが BCP 47 風の構文に合致するかを判定する。
///
/// 先頭サブタグ（言語）は 2〜3 文字の ASCII 英字、後続サブタグは 1〜8 文字の ASCII 英数字とし、
/// ハイフン区切りで空サブタグを含まないことを要求する。実在性は検証しない。
fn is_well_formed_locale(code: &str) -> bool {
  let mut subtags = code.split('-');
  let Some(language) = subtags.next() else {
    return false;
  };
  if !(2..=3).contains(&language.len()) || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
    return false;
  }
  for subtag in subtags {
    if !(1..=8).contains(&subtag.len()) || !subtag.bytes().all(|b| b.is_ascii_alphanumeric()) {
      return false;
    }
  }
  return true;
}

/// ロケールコードを BCP 47 の標準形（言語=小文字 / 用字=先頭大文字 / 地域=大文字）へ正規化する。
///
/// 構文が [`is_well_formed_locale`] を満たす前提で、各サブタグの大文字小文字だけを整える。
/// - 言語（先頭サブタグ）: 小文字（例 `JA` → `ja`）
/// - 用字（4 文字英字）: 先頭大文字 + 残り小文字（例 `hant` → `Hant`）
/// - 地域（2 文字英字）: 大文字（例 `jp` → `JP`）
/// - それ以外（3 桁地域コード・variant 等）: そのまま
fn normalize_locale_code(code: &str) -> String {
  let mut out = String::with_capacity(code.len());
  for (index, subtag) in code.split('-').enumerate() {
    if index > 0 {
      out.push('-');
    }
    if index == 0 {
      out.extend(subtag.chars().map(|c| c.to_ascii_lowercase()));
    } else if subtag.len() == 4 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
      out.extend(subtag.chars().enumerate().map(|(i, c)| {
        if i == 0 {
          c.to_ascii_uppercase()
        } else {
          c.to_ascii_lowercase()
        }
      }));
    } else if subtag.len() == 2 && subtag.bytes().all(|b| b.is_ascii_alphabetic()) {
      out.extend(subtag.chars().map(|c| c.to_ascii_uppercase()));
    } else {
      out.push_str(subtag);
    }
  }
  return out;
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
    // Arrange / Act / Assert — 空・短すぎる言語・アンダースコア・空サブタグ・非英数字
    for bad in ["", "j", "ja_JP", "ja-", "ja JP", "english"] {
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
  fn normalize_keeps_none_locale() {
    // Arrange / Act / Assert — locale が None なら何もしない
    let mut style = ReferenceStyle::default();
    style.normalize();
    assert!(style.locale.is_none());
  }
}
