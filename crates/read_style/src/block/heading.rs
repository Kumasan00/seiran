//! 見出し要素（part / chapter / section …）のスタイル設定型。

use std::collections::HashMap;

use garde::Validate;
use serde::{Deserialize, Deserializer, Serialize, de::Error};
use types::{FontKind, HeadingLevel};

use crate::per_level::PerLevel;

/// 見出し要素のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct HeadingStyle {
  /// 見出しの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1))]
  pub format: String,
  /// 見出しテキストのフォントサイズ（pt）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub font_size: f32,
  /// 見出しブロックの下余白
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub bottom_margin: f32,
  /// 見出しの直前で改ページするか
  pub page_break_before: bool,
  /// 見出しの直後で改ページするか
  pub page_break_after: bool,
  /// 見出しテキストのフォント種別
  pub font_kind: FontKind,
}

impl Default for HeadingStyle {
  /// `HeadingLevel::Section` 相当の汎用デフォルト（`{number} {title}` / 20pt / 太字セリフ）。
  ///
  /// レベルごとの差分は [`default_for_level`] が与える。デシリアライズ時の `#[serde(default)]`
  /// で部分指定をサポートするために必要。
  fn default() -> Self {
    return Self {
      format: "{number} {title}".to_string(),
      font_size: 20.0,
      bottom_margin: 10.0,
      page_break_before: false,
      page_break_after: false,
      font_kind: FontKind::SerifBold,
    };
  }
}

/// 見出しレベル全 6 つに対応する [`PerLevel<HeadingStyle>`] のデフォルトを生成する。
///
/// `HeadingStyle::default` で共通項を埋めつつ、レベル固有の値（フォントサイズ・テンプレ・改頁）だけ
/// このテーブルで上書きする。`Style::default()` の手書き重複を 1 箇所に集約する。
#[must_use]
pub fn default_per_level() -> PerLevel<HeadingStyle> { return PerLevel::from_fn(default_for_level); }

/// `Style::heading` 用のレベル別デフォルトを尊重するカスタムデシリアライザ。
///
/// 通常の `PerLevel<HeadingStyle>` の `Deserialize` は欠落キーを `HeadingStyle::default()`
/// で埋めるが、見出しはレベルごとに異なる既定値（Chapter: 25pt + `page_break_before` 等）が
/// あるため、欠落キーを [`default_for_level`] で埋める専用ルートを用意する。
///
/// `#[serde(deserialize_with = "deserialize_per_level")]` で `Style::heading` に取り付けて使う。
///
/// # Errors
///
/// - TOML がテーブルとしてデシリアライズできない場合
/// - 未知の見出しレベル名が含まれる場合
pub fn deserialize_per_level<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PerLevel<HeadingStyle>, D::Error> {
  let mut map: HashMap<HeadingLevel, HeadingStyle> = HashMap::deserialize(deserializer)?;
  let array = HeadingLevel::ALL.map(|level| map.remove(&level).unwrap_or_else(|| default_for_level(level)));
  if !map.is_empty() {
    let unknown: Vec<_> = map.keys().map(|level| level.command_name()).collect();
    return Err(D::Error::custom(format!("未知の見出しレベル: {unknown:?}")));
  }
  return Ok(PerLevel::new(array));
}

/// 指定レベルの [`HeadingStyle`] デフォルトを返す。
///
/// テンプレ・フォントサイズ・改頁の差分のみテーブル化し、フォント種別等は
/// `HeadingStyle::default` の値をそのまま継承する。
#[must_use]
pub fn default_for_level(level: HeadingLevel) -> HeadingStyle {
  let mut style = HeadingStyle::default();
  match level {
    HeadingLevel::Part => {
      style.format = "Part {number}: {title}".to_string();
      style.font_size = 40.0;
      style.bottom_margin = 20.0;
      style.page_break_before = true;
      style.page_break_after = true;
    },
    HeadingLevel::Chapter => {
      style.format = "Chapter {number}: {title}".to_string();
      style.font_size = 25.0;
      style.bottom_margin = 15.0;
      style.page_break_before = true;
    },
    HeadingLevel::Section => {
      style.font_size = 20.0;
    },
    HeadingLevel::Subsection => {
      style.font_size = 16.0;
    },
    HeadingLevel::Paragraph => {
      style.font_size = 14.0;
      style.bottom_margin = 5.0;
    },
    HeadingLevel::Subparagraph => {
      style.font_size = 12.0;
      style.bottom_margin = 5.0;
    },
  }
  return style;
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{FontKind, HeadingLevel};

  use super::{HeadingStyle, default_for_level, default_per_level};

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(HeadingStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    // Arrange
    let heading = HeadingStyle {
      format: String::new(),
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let heading = HeadingStyle {
      font_size: 0.0,
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    // Arrange
    let heading = HeadingStyle {
      bottom_margin: -0.1,
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_accepts_alternative_font_kind() {
    // Arrange
    let heading = HeadingStyle {
      font_kind: FontKind::SansSerifBold,
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(heading.validate().is_ok());
  }

  #[test]
  fn default_for_level_uses_distinct_font_sizes() {
    // Arrange / Act
    let part = default_for_level(HeadingLevel::Part);
    let section = default_for_level(HeadingLevel::Section);
    let subparagraph = default_for_level(HeadingLevel::Subparagraph);

    // Assert
    assert!(part.font_size > section.font_size);
    assert!(section.font_size > subparagraph.font_size);
  }

  #[test]
  fn default_per_level_has_part_page_break_after() {
    // Arrange / Act
    let per_level = default_per_level();

    // Assert
    assert!(per_level[HeadingLevel::Part].page_break_after);
    assert!(!per_level[HeadingLevel::Section].page_break_after);
  }

  #[test]
  fn default_per_level_default_template_for_section() {
    // Arrange / Act
    let per_level = default_per_level();

    // Assert
    assert_eq!(per_level[HeadingLevel::Section].format, "{number} {title}");
    assert!(per_level[HeadingLevel::Chapter].format.starts_with("Chapter"));
  }
}
