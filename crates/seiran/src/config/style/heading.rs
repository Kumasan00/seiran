//! 見出し要素（part / chapter / section …）のスタイル設定型。
//!
//! `[heading.<level>]` の指定を [`default_for_level`] に重ねて解釈する。

use std::ops::{Index, IndexMut};

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::model::{
  FontKind, HeadingLevel,
  length::{Length, non_negative, positive},
};

/// 見出しレベル全 6 つに対応するスタイル設定。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "HeadingStylesTable")]
pub struct HeadingStyles {
  /// `[heading.part]`
  pub part: HeadingStyle,
  /// `[heading.chapter]`
  pub chapter: HeadingStyle,
  /// `[heading.section]`
  pub section: HeadingStyle,
  /// `[heading.subsection]`
  pub subsection: HeadingStyle,
  /// `[heading.paragraph]`
  pub paragraph: HeadingStyle,
  /// `[heading.subparagraph]`
  pub subparagraph: HeadingStyle,
}

impl Default for HeadingStyles {
  fn default() -> Self {
    return Self {
      part: default_for_level(HeadingLevel::Part),
      chapter: default_for_level(HeadingLevel::Chapter),
      section: default_for_level(HeadingLevel::Section),
      subsection: default_for_level(HeadingLevel::Subsection),
      paragraph: default_for_level(HeadingLevel::Paragraph),
      subparagraph: default_for_level(HeadingLevel::Subparagraph),
    };
  }
}

impl Index<HeadingLevel> for HeadingStyles {
  type Output = HeadingStyle;

  fn index(&self, level: HeadingLevel) -> &HeadingStyle {
    return match level {
      HeadingLevel::Part => &self.part,
      HeadingLevel::Chapter => &self.chapter,
      HeadingLevel::Section => &self.section,
      HeadingLevel::Subsection => &self.subsection,
      HeadingLevel::Paragraph => &self.paragraph,
      HeadingLevel::Subparagraph => &self.subparagraph,
    };
  }
}

impl IndexMut<HeadingLevel> for HeadingStyles {
  fn index_mut(&mut self, level: HeadingLevel) -> &mut HeadingStyle {
    return match level {
      HeadingLevel::Part => &mut self.part,
      HeadingLevel::Chapter => &mut self.chapter,
      HeadingLevel::Section => &mut self.section,
      HeadingLevel::Subsection => &mut self.subsection,
      HeadingLevel::Paragraph => &mut self.paragraph,
      HeadingLevel::Subparagraph => &mut self.subparagraph,
    };
  }
}

impl HeadingStyles {
  /// 各レベルにレベル名を添えて走査するイテレータ。
  pub fn iter_with_level(&self) -> impl Iterator<Item = (HeadingLevel, &HeadingStyle)> {
    return [
      (HeadingLevel::Part, &self.part),
      (HeadingLevel::Chapter, &self.chapter),
      (HeadingLevel::Section, &self.section),
      (HeadingLevel::Subsection, &self.subsection),
      (HeadingLevel::Paragraph, &self.paragraph),
      (HeadingLevel::Subparagraph, &self.subparagraph),
    ]
    .into_iter();
  }
}

/// 見出し要素のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct HeadingStyle {
  /// 見出しの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1), custom(crate::config::style::placeholder::heading_format))]
  pub format: String,
  /// 見出しテキストのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 見出しブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 見出しの直前で改ページするか
  pub page_break_before: bool,
  /// 見出しの直後で改ページするか
  pub page_break_after: bool,
  /// 見出しテキストのフォント種別
  pub font_kind: FontKind,
}

impl Default for HeadingStyle {
  fn default() -> Self {
    return Self {
      format: "{number} {title}".to_string(),
      font_size: Length::pt(20.0),
      bottom_margin: Length::pt(10.0),
      page_break_before: false,
      page_break_after: false,
      font_kind: FontKind::SerifBold,
    };
  }
}

/// `[heading]` テーブル全体の TOML スキーマ。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct HeadingStylesTable {
  /// `part` レベルの上書き
  part: HeadingStyleOverride,
  /// `chapter` レベルの上書き
  chapter: HeadingStyleOverride,
  /// `section` レベルの上書き
  section: HeadingStyleOverride,
  /// `subsection` レベルの上書き
  subsection: HeadingStyleOverride,
  /// `paragraph` レベルの上書き
  paragraph: HeadingStyleOverride,
  /// `subparagraph` レベルの上書き
  subparagraph: HeadingStyleOverride,
}

impl From<HeadingStylesTable> for HeadingStyles {
  fn from(table: HeadingStylesTable) -> Self {
    let build = |level: HeadingLevel, over: HeadingStyleOverride| -> HeadingStyle {
      let mut style = default_for_level(level);
      over.apply(&mut style);
      return style;
    };
    return Self {
      part: build(HeadingLevel::Part, table.part),
      chapter: build(HeadingLevel::Chapter, table.chapter),
      section: build(HeadingLevel::Section, table.section),
      subsection: build(HeadingLevel::Subsection, table.subsection),
      paragraph: build(HeadingLevel::Paragraph, table.paragraph),
      subparagraph: build(HeadingLevel::Subparagraph, table.subparagraph),
    };
  }
}

/// [`HeadingStyle`] の各フィールドを `Option<_>` で覆った差分指定型。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct HeadingStyleOverride {
  /// 見出しの書式テンプレート
  pub format: Option<String>,
  /// 見出しテキストのフォントサイズ
  pub font_size: Option<Length>,
  /// 見出しブロックの下余白
  pub bottom_margin: Option<Length>,
  /// 見出しの直前で改ページするか
  pub page_break_before: Option<bool>,
  /// 見出しの直後で改ページするか
  pub page_break_after: Option<bool>,
  /// 見出しテキストのフォント種別
  pub font_kind: Option<FontKind>,
}

impl HeadingStyleOverride {
  /// 自身の `Some` 値で `target` のフィールドを上書きする。
  fn apply(&self, target: &mut HeadingStyle) {
    if let Some(format) = &self.format {
      target.format.clone_from(format);
    }
    if let Some(font_size) = self.font_size {
      target.font_size = font_size;
    }
    if let Some(bottom_margin) = self.bottom_margin {
      target.bottom_margin = bottom_margin;
    }
    if let Some(page_break_before) = self.page_break_before {
      target.page_break_before = page_break_before;
    }
    if let Some(page_break_after) = self.page_break_after {
      target.page_break_after = page_break_after;
    }
    if let Some(font_kind) = self.font_kind {
      target.font_kind = font_kind;
    }
  }
}

/// 指定レベルの [`HeadingStyle`] デフォルトを返す。
#[must_use]
pub fn default_for_level(level: HeadingLevel) -> HeadingStyle {
  let mut style = HeadingStyle::default();
  match level {
    HeadingLevel::Part => {
      style.format = "Part {number}: {title}".to_string();
      style.font_size = Length::pt(40.0);
      style.bottom_margin = Length::pt(20.0);
      style.page_break_before = true;
      style.page_break_after = true;
    },
    HeadingLevel::Chapter => {
      style.format = "Chapter {number}: {title}".to_string();
      style.font_size = Length::pt(25.0);
      style.bottom_margin = Length::pt(15.0);
      style.page_break_before = true;
    },
    HeadingLevel::Section => {
      style.font_size = Length::pt(20.0);
    },
    HeadingLevel::Subsection => {
      style.font_size = Length::pt(16.0);
    },
    HeadingLevel::Paragraph => {
      style.font_size = Length::pt(14.0);
      style.bottom_margin = Length::pt(5.0);
    },
    HeadingLevel::Subparagraph => {
      style.font_size = Length::pt(12.0);
      style.bottom_margin = Length::pt(5.0);
    },
  }
  return style;
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{HeadingStyle, HeadingStyles, default_for_level};
  use crate::model::{FontKind, HeadingLevel, length::Length};

  #[test]
  fn validate_rejects_unknown_placeholder_in_format() {
    // Arrange
    let style = HeadingStyle {
      format: "{nubmer} {title}".to_string(),
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  /// `HeadingStyles` を TOML から `[heading.<level>]` 配下に書く形でテストするための薄いラッパ。
  /// 本番では `Style.heading` が同形でこの型を保持する。
  #[derive(Debug, serde::Deserialize)]
  struct HeadingWrapper {
    heading: HeadingStyles,
  }

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
      font_size: Length::pt(0.0),
      ..HeadingStyle::default()
    };

    // Act / Assert
    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    // Arrange
    let heading = HeadingStyle {
      bottom_margin: Length::pt(-0.1),
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
  fn default_styles_has_part_page_break_after() {
    // Arrange / Act
    let styles = HeadingStyles::default();

    // Assert
    assert!(styles[HeadingLevel::Part].page_break_after);
    assert!(!styles[HeadingLevel::Section].page_break_after);
  }

  #[test]
  fn default_styles_default_template_for_section() {
    // Arrange / Act
    let styles = HeadingStyles::default();

    // Assert
    assert_eq!(styles[HeadingLevel::Section].format, "{number} {title}");
    assert!(styles[HeadingLevel::Chapter].format.starts_with("Chapter"));
  }

  #[test]
  fn heading_styles_rejects_unknown_level_key() {
    // Arrange
    let toml = "
[heading.unknown_level]
font_size = \"12pt\"
";

    // Act
    let result: Result<HeadingWrapper, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知のレベル名は拒否されるべき: {result:?}");
  }

  #[test]
  fn heading_styles_rejects_base_scalar_keys() {
    // Arrange
    let toml = "
[heading]
font_kind = \"sans_serif_bold\"
";

    // Act
    let result: Result<HeadingWrapper, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "[heading] 直下のスカラー指定は拒否されるべき: {result:?}");
  }

  #[test]
  fn heading_styles_partial_level_keeps_other_defaults() {
    // Arrange
    let toml = "
[heading.section]
format = \"§ {number} {title}\"
";

    // Act
    let wrapper: HeadingWrapper = toml::from_str(toml).unwrap();
    let styles = wrapper.heading;

    // Assert
    assert_eq!(styles[HeadingLevel::Section].format, "§ {number} {title}");
    assert!((styles[HeadingLevel::Section].font_size.to_pt() - 20.0).abs() < f32::EPSILON);
    assert!(styles[HeadingLevel::Part].page_break_after);
    assert_eq!(styles[HeadingLevel::Part].font_kind, FontKind::SerifBold);
  }

  #[test]
  fn iter_with_level_yields_all_six_levels_in_order() {
    // Arrange
    let styles = HeadingStyles::default();

    // Act
    let levels: Vec<HeadingLevel> = styles.iter_with_level().map(|(level, _)| return level).collect();

    // Assert
    assert_eq!(levels, HeadingLevel::ALL.to_vec());
  }
}
