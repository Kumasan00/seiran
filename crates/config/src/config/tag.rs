//! OpenType タグ文字列の検証・構築の単一情報源
//!
//! `script` / `ot_language` / フィーチャー / バリアブル軸の各タグは、いずれも 4 バイトの
//! OpenType タグ（`[u8; 4]`）として表現されますが、許容する文字種・長さ・正規化の規則が
//! 異なります。このモジュールはそれぞれの規則を **1 つの失敗しうるコンストラクタ** に集約し、
//! 「妥当性の検査」と「`[u8; 4]` への変換」を同じ関数で行います。
//!
//! これにより、検証（旧 `garde` カスタムバリデータ）と構築（旧 `four_byte_tag` /
//! `normalize_ot_language_tag`）が別々にタグ規則を二重定義してドリフトする問題、および
//! 検証済み前提の `unwrap()` が長さ不正入力でパニックする問題を回避します。
//!
//! | 関数 | 用途 | 規則 | 正規化 |
//! |------|------|------|--------|
//! | [`parse_script_tag`] | `script` | 4 文字 ASCII アルファベット | なし（case 保持） |
//! | [`parse_ot_language_tag`] | `ot_language` | 3-4 文字 ASCII alphanumeric | 大文字化 + 末尾空白パディング |
//! | [`parse_opentype_tag`] | フィーチャー / 軸名 | 4 バイト ASCII | なし（case 保持） |

use thiserror::Error;

/// OpenType タグ文字列の検証に失敗した理由。
///
/// `read_config` 内部の検証・変換でのみ使用し、最終的に
/// [`crate::config::ConfigValidationError::Field`] の `message` へ畳み込まれます。
#[derive(Debug, Error)]
pub(crate) enum TagError {
  /// `script` タグが 4 文字 ASCII アルファベットでない
  #[error("OpenType script タグは 4 文字の ASCII アルファベットである必要があります")]
  Script,
  /// `ot_language` タグが 3-4 文字 ASCII alphanumeric でない
  #[error("OpenType language タグは 3-4 文字の ASCII alphanumeric である必要があります")]
  OtLanguage,
  /// フィーチャー / 軸名タグが 4 バイト ASCII でない
  #[error("OpenType タグは 4 文字の ASCII である必要があります")]
  OpenType,
}

/// `script` タグ（4 文字 ASCII アルファベット）を検証して `[u8; 4]` に変換します。
///
/// case は正規化せずユーザ指定をそのまま保持します（例: `"latn"` / `"Latn"` / `"LATN"` は
/// それぞれ異なるバイト列になります）。harfrust 側の case 正規化やフォント実態との突合せは
/// `font` クレートが担当します。
pub(crate) fn parse_script_tag(value: &str) -> Result<[u8; 4], TagError> {
  if value.len() == 4 && value.bytes().all(|b| return b.is_ascii_alphabetic()) {
    return Ok(to_array(value));
  }
  return Err(TagError::Script);
}

/// `ot_language` タグ（3-4 文字 ASCII alphanumeric）を検証し、4 バイトに正規化します。
///
/// OpenType 言語システムタグの慣習に従い、大文字化したうえで 4 バイト未満は末尾を空白で
/// パディングします（例: `"JAN"` → `b"JAN "`、`"eng"` → `b"ENG "`）。
pub(crate) fn parse_ot_language_tag(value: &str) -> Result<[u8; 4], TagError> {
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
pub(crate) fn parse_opentype_tag(value: &str) -> Result<[u8; 4], TagError> {
  if value.len() == 4 && value.is_ascii() {
    return Ok(to_array(value));
  }
  return Err(TagError::OpenType);
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
  use super::{parse_opentype_tag, parse_ot_language_tag, parse_script_tag};

  #[test]
  fn parse_script_tag_preserves_case_for_four_ascii_letters() {
    // Arrange / Act / Assert: 4 文字 ASCII アルファベットは case 保持で受理
    for tag in ["latn", "Latn", "LATN", "kana", "DFLT"] {
      assert_eq!(parse_script_tag(tag).unwrap(), to_bytes(tag), "{tag}");
    }
  }

  #[test]
  fn parse_script_tag_rejects_wrong_length_or_non_alpha() {
    // Arrange / Act / Assert: 長さ違い・数字・非 ASCII は拒否
    for tag in ["kan", "kanaa", "kan1", "ka一"] {
      assert!(parse_script_tag(tag).is_err(), "{tag}");
    }
  }

  #[test]
  fn parse_ot_language_tag_uppercases_and_pads_to_four_bytes() {
    // Arrange / Act / Assert: 3 文字は末尾スペース、小文字は大文字化
    assert_eq!(parse_ot_language_tag("JAN").unwrap(), *b"JAN ");
    assert_eq!(parse_ot_language_tag("eng").unwrap(), *b"ENG ");
    assert_eq!(parse_ot_language_tag("DEUT").unwrap(), *b"DEUT");
  }

  #[test]
  fn parse_ot_language_tag_rejects_invalid_length_or_non_alphanumeric() {
    // Arrange / Act / Assert: 2 文字・5 文字・非 alphanumeric は拒否
    for tag in ["JA", "JAPAN", "J!N"] {
      assert!(parse_ot_language_tag(tag).is_err(), "{tag}");
    }
  }

  #[test]
  fn parse_opentype_tag_accepts_four_ascii_including_digits() {
    // Arrange / Act / Assert: 数字を含む 4 文字 ASCII（ss01 等）も受理
    for tag in ["liga", "ss01", "wght", "smcp"] {
      assert_eq!(parse_opentype_tag(tag).unwrap(), to_bytes(tag), "{tag}");
    }
  }

  #[test]
  fn parse_opentype_tag_rejects_wrong_length_or_non_ascii() {
    // Arrange / Act / Assert: 長さ違い・非 ASCII は拒否
    for tag in ["lig", "ligaa", "li一"] {
      assert!(parse_opentype_tag(tag).is_err(), "{tag}");
    }
  }

  /// テスト用: 4 文字 ASCII 文字列を `[u8; 4]` へ変換します。
  fn to_bytes(s: &str) -> [u8; 4] {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(s.as_bytes());
    return bytes;
  }
}
