//! OpenType タグ文字列の検証・構築の単一情報源

use thiserror::Error;

/// OpenType タグ文字列の検証に失敗した理由。
#[derive(Debug, Error)]
pub(super) enum TagError {
  /// `script` タグが 4 文字 ASCII アルファベットでない
  #[error("OpenType script タグは 4 文字の ASCII アルファベットである必要があります")]
  Script,
  /// `ot_language` タグが 3-4 文字 ASCII alphanumeric でない
  #[error("OpenType language タグは 3-4 文字の ASCII alphanumeric である必要があります")]
  OtLanguage,
  /// フィーチャー / 軸名タグが 4 バイト ASCII でない
  #[error("OpenType タグは 4 文字の ASCII である必要があります")]
  FeatureOrAxis,
}

/// `script` タグ（4 文字 ASCII アルファベット）を検証して `[u8; 4]` に変換します。
///
/// case は正規化せずユーザ指定をそのまま保持します（例: `"latn"` / `"Latn"` / `"LATN"` は
/// それぞれ異なるバイト列になります）。
pub(crate) fn parse_script(value: &str) -> Result<[u8; 4], TagError> {
  if value.len() == 4 && value.bytes().all(|b| return b.is_ascii_alphabetic()) {
    return Ok(to_array(value));
  }
  return Err(TagError::Script);
}

/// `ot_language` タグ（3-4 文字 ASCII alphanumeric）を検証し、4 バイトに正規化します。
///
/// OpenType 言語システムタグの慣習に従い、大文字化したうえで 4 バイト未満は末尾を空白で
/// パディングします（例: `"JAN"` → `b"JAN "`、`"eng"` → `b"ENG "`）。
pub(crate) fn parse_ot_language(value: &str) -> Result<[u8; 4], TagError> {
  if (3..=4).contains(&value.len()) && value.bytes().all(|b| return b.is_ascii_alphanumeric()) {
    let mut bytes = [b' '; 4];
    for (i, b) in value.bytes().enumerate() {
      bytes[i] = b.to_ascii_uppercase();
    }
    return Ok(bytes);
  }
  return Err(TagError::OtLanguage);
}

/// フィーチャー / バリアブル軸名のタグ（4 バイト ASCII）を検証して `[u8; 4]` に変換します。
///
/// アルファベットに限定せず ASCII 全般を許可します（例: `"ss01"` のような数字を含む
/// ストイリスティックセットタグも有効）。case は保持します。
pub(crate) fn parse_feature_or_axis(value: &str) -> Result<[u8; 4], TagError> {
  if value.len() == 4 && value.is_ascii() {
    return Ok(to_array(value));
  }
  return Err(TagError::FeatureOrAxis);
}

/// バイト長 4 が保証された `&str` を `[u8; 4]` へコピーします。
///
/// 呼び出し側で `value.len() == 4` を検証済みであることが前提です。
fn to_array(value: &str) -> [u8; 4] {
  let mut bytes = [0u8; 4];
  bytes.copy_from_slice(value.as_bytes());
  return bytes;
}

#[cfg(test)]
mod tests {
  use super::{parse_feature_or_axis, parse_ot_language, parse_script};

  #[test]
  fn parse_script_preserves_case_for_four_ascii_letters() {
    for tag in ["latn", "Latn", "LATN", "kana", "DFLT"] {
      assert_eq!(parse_script(tag).unwrap(), to_bytes(tag), "{tag}");
    }
  }

  #[test]
  fn parse_script_rejects_wrong_length_or_non_alpha() {
    for tag in ["kan", "kanaa", "kan1", "ka一"] {
      assert!(parse_script(tag).is_err(), "{tag}");
    }
  }

  #[test]
  fn parse_ot_language_uppercases_and_pads_to_four_bytes() {
    assert_eq!(parse_ot_language("JAN").unwrap(), *b"JAN ");
    assert_eq!(parse_ot_language("eng").unwrap(), *b"ENG ");
    assert_eq!(parse_ot_language("DEUT").unwrap(), *b"DEUT");
  }

  #[test]
  fn parse_ot_language_rejects_invalid_length_or_non_alphanumeric() {
    for tag in ["JA", "JAPAN", "J!N"] {
      assert!(parse_ot_language(tag).is_err(), "{tag}");
    }
  }

  #[test]
  fn parse_feature_or_axis_accepts_four_ascii_including_digits() {
    for tag in ["liga", "ss01", "wght", "smcp"] {
      assert_eq!(parse_feature_or_axis(tag).unwrap(), to_bytes(tag), "{tag}");
    }
  }

  #[test]
  fn parse_feature_or_axis_rejects_wrong_length_or_non_ascii() {
    for tag in ["lig", "ligaa", "li一"] {
      assert!(parse_feature_or_axis(tag).is_err(), "{tag}");
    }
  }

  /// テスト用: 4 文字 ASCII 文字列を `[u8; 4]` へ変換します。
  fn to_bytes(s: &str) -> [u8; 4] {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(s.as_bytes());
    return bytes;
  }
}
