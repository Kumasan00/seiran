//! 見出し要素（part / chapter / section …）のスタイル設定型。
//!
//! `[heading.<level>]` の指定を [`HeadingStyles::default`] のレベル別既定に重ねて解釈する
//! （定理・カウンタと同じ 2 レイヤーマージ）。

use std::ops::Index;

use garde::Validate;
use serde::Deserialize;

use crate::{
  document::{FontKind, HeadingLevel},
  length::{Length, non_negative, positive},
  style::NumberTitleTemplate,
};

/// 見出しレベル全 6 つに対応するスタイル設定。
///
/// TOML からは [`HeadingStylesTable`]（各エントリが差分指定 [`HeadingStyleOverride`]）として読み、
/// [`HeadingStyles::default`] のレベル別既定へ重ねて解決済みの値を作る。
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(from = "HeadingStylesTable")]
pub(crate) struct HeadingStyles {
  /// `[heading.part]`
  #[garde(dive)]
  pub part: HeadingStyle,
  /// `[heading.chapter]`
  #[garde(dive)]
  pub chapter: HeadingStyle,
  /// `[heading.section]`
  #[garde(dive)]
  pub section: HeadingStyle,
  /// `[heading.subsection]`
  #[garde(dive)]
  pub subsection: HeadingStyle,
  /// `[heading.paragraph]`
  #[garde(dive)]
  pub paragraph: HeadingStyle,
  /// `[heading.subparagraph]`
  #[garde(dive)]
  pub subparagraph: HeadingStyle,
}

impl Default for HeadingStyles {
  fn default() -> Self {
    return Self {
      part: HeadingStyle {
        format: NumberTitleTemplate::parse("Part {number}: {title}"),
        font_size: Length::pt(40.0),
        bottom_margin: Length::pt(20.0),
        page_break_before: true,
        page_break_after: true,
        ..HeadingStyle::default()
      },
      chapter: HeadingStyle {
        format: NumberTitleTemplate::parse("Chapter {number}: {title}"),
        font_size: Length::pt(25.0),
        bottom_margin: Length::pt(15.0),
        page_break_before: true,
        ..HeadingStyle::default()
      },
      section: HeadingStyle {
        font_size: Length::pt(20.0),
        ..HeadingStyle::default()
      },
      subsection: HeadingStyle {
        font_size: Length::pt(16.0),
        ..HeadingStyle::default()
      },
      paragraph: HeadingStyle {
        font_size: Length::pt(14.0),
        bottom_margin: Length::pt(5.0),
        ..HeadingStyle::default()
      },
      subparagraph: HeadingStyle {
        font_size: Length::pt(12.0),
        bottom_margin: Length::pt(5.0),
        ..HeadingStyle::default()
      },
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

/// 見出し要素のスタイル設定
///
/// TOML のスキーマは [`HeadingStyleOverride`]。
#[derive(Debug, Clone, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct HeadingStyle {
  /// 見出しの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(dive)]
  pub format: NumberTitleTemplate,
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

/// レベル別既定（[`HeadingStyles::default`]）が共通に使う基底。
impl Default for HeadingStyle {
  fn default() -> Self {
    return Self {
      format: NumberTitleTemplate::parse("{number} {title}"),
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
    let defaults = Self::default();
    return Self {
      part: table.part.apply(defaults.part),
      chapter: table.chapter.apply(defaults.chapter),
      section: table.section.apply(defaults.section),
      subsection: table.subsection.apply(defaults.subsection),
      paragraph: table.paragraph.apply(defaults.paragraph),
      subparagraph: table.subparagraph.apply(defaults.subparagraph),
    };
  }
}

/// [`HeadingStyle`] の各フィールドを `Option<_>` で覆った差分指定型（`[heading.<level>]` の TOML スキーマ）。
///
/// `None` のフィールドはレベル別既定のまま残す。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct HeadingStyleOverride {
  /// 見出しの書式テンプレート
  format: Option<NumberTitleTemplate>,
  /// 見出しテキストのフォントサイズ
  font_size: Option<Length>,
  /// 見出しブロックの下余白
  bottom_margin: Option<Length>,
  /// 見出しの直前で改ページするか
  page_break_before: Option<bool>,
  /// 見出しの直後で改ページするか
  page_break_after: Option<bool>,
  /// 見出しテキストのフォント種別
  font_kind: Option<FontKind>,
}

impl HeadingStyleOverride {
  /// 自身の `Some` 値で `base` のフィールドを置き換えた値を返す。
  ///
  /// `self` の分割と戻り値のリテラルがどちらも `..` 無しなので、差分指定型・解決済み型のどちらに
  /// フィールドを足しても、ここで扱いを決めるまでコンパイルが通らない。
  fn apply(self, base: HeadingStyle) -> HeadingStyle {
    let Self {
      format,
      font_size,
      bottom_margin,
      page_break_before,
      page_break_after,
      font_kind,
    } = self;
    return HeadingStyle {
      format: format.unwrap_or(base.format),
      font_size: font_size.unwrap_or(base.font_size),
      bottom_margin: bottom_margin.unwrap_or(base.bottom_margin),
      page_break_before: page_break_before.unwrap_or(base.page_break_before),
      page_break_after: page_break_after.unwrap_or(base.page_break_after),
      font_kind: font_kind.unwrap_or(base.font_kind),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{HeadingStyle, HeadingStyles};
  use crate::{
    document::{FontKind, HeadingLevel},
    length::Length,
    style::NumberTitleTemplate,
  };

  #[test]
  fn validate_rejects_unknown_placeholder_in_format() {
    let style = HeadingStyle {
      format: NumberTitleTemplate::parse("{nubmer} {title}"),
      ..HeadingStyle::default()
    };

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
    assert!(HeadingStyle::default().validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_format() {
    let heading = HeadingStyle {
      format: NumberTitleTemplate::parse(""),
      ..HeadingStyle::default()
    };

    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let heading = HeadingStyle {
      font_size: Length::pt(0.0),
      ..HeadingStyle::default()
    };

    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_bottom_margin() {
    let heading = HeadingStyle {
      bottom_margin: Length::pt(-0.1),
      ..HeadingStyle::default()
    };

    assert!(heading.validate().is_err());
  }

  #[test]
  fn validate_accepts_alternative_font_kind() {
    let heading = HeadingStyle {
      font_kind: FontKind::SansSerifBold,
      ..HeadingStyle::default()
    };

    assert!(heading.validate().is_ok());
  }

  #[test]
  fn default_styles_use_distinct_font_sizes() {
    let styles = HeadingStyles::default();

    assert!(styles[HeadingLevel::Part].font_size > styles[HeadingLevel::Section].font_size);
    assert!(styles[HeadingLevel::Section].font_size > styles[HeadingLevel::Subparagraph].font_size);
  }

  #[test]
  fn default_styles_has_part_page_break_after() {
    let styles = HeadingStyles::default();

    assert!(styles[HeadingLevel::Part].page_break_after);
    assert!(!styles[HeadingLevel::Section].page_break_after);
  }

  #[test]
  fn default_styles_default_template_for_section() {
    let styles = HeadingStyles::default();

    assert_eq!(styles[HeadingLevel::Section].format.as_str(), "{number} {title}");
    assert!(styles[HeadingLevel::Chapter].format.as_str().starts_with("Chapter"));
  }

  #[test]
  fn default_styles_match_level_defaults() {
    // (level, format, font_size_pt, bottom_margin_pt, page_break_before, page_break_after)
    let expected: [(HeadingLevel, &str, f32, f32, bool, bool); 6] = [
      (HeadingLevel::Part, "Part {number}: {title}", 40.0, 20.0, true, true),
      (HeadingLevel::Chapter, "Chapter {number}: {title}", 25.0, 15.0, true, false),
      (HeadingLevel::Section, "{number} {title}", 20.0, 10.0, false, false),
      (HeadingLevel::Subsection, "{number} {title}", 16.0, 10.0, false, false),
      (HeadingLevel::Paragraph, "{number} {title}", 14.0, 5.0, false, false),
      (HeadingLevel::Subparagraph, "{number} {title}", 12.0, 5.0, false, false),
    ];
    let styles = HeadingStyles::default();

    for (level, format, font_size, bottom_margin, before, after) in expected {
      let style = &styles[level];
      assert_eq!(style.format.as_str(), format, "{level:?} の書式");
      assert!((style.font_size.to_pt() - font_size).abs() < f32::EPSILON, "{level:?} の文字サイズ");
      assert!((style.bottom_margin.to_pt() - bottom_margin).abs() < f32::EPSILON, "{level:?} の下余白");
      assert_eq!(style.page_break_before, before, "{level:?} の前改ページ");
      assert_eq!(style.page_break_after, after, "{level:?} の後改ページ");
      assert_eq!(style.font_kind, FontKind::SerifBold, "{level:?} の書体");
    }
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
    assert_eq!(styles[HeadingLevel::Section].format.as_str(), "§ {number} {title}");
    assert!((styles[HeadingLevel::Section].font_size.to_pt() - 20.0).abs() < f32::EPSILON);
    assert!(styles[HeadingLevel::Part].page_break_after);
    assert_eq!(styles[HeadingLevel::Part].font_kind, FontKind::SerifBold);
  }

  #[test]
  fn heading_styles_full_level_overrides_every_key() {
    // Arrange — 1 レベルに 6 キー全部を書いた形（同型のフィールド同士の取り違えを検出する）
    let toml = "
[heading.subsection]
format = \"{title}\"
font_size = \"13pt\"
bottom_margin = \"4pt\"
page_break_before = true
page_break_after = true
font_kind = \"sans_serif_bold\"
";

    // Act
    let wrapper: HeadingWrapper = toml::from_str(toml).unwrap();
    let subsection = &wrapper.heading[HeadingLevel::Subsection];

    // Assert
    assert_eq!(subsection.format.as_str(), "{title}");
    assert_eq!(subsection.font_size, Length::pt(13.0));
    assert_eq!(subsection.bottom_margin, Length::pt(4.0));
    assert!(subsection.page_break_before);
    assert!(subsection.page_break_after);
    assert_eq!(subsection.font_kind, FontKind::SansSerifBold);
  }

  #[test]
  fn indexing_returns_matching_field() {
    let styles = HeadingStyles::default();

    assert!(std::ptr::eq(&raw const styles[HeadingLevel::Chapter], &raw const styles.chapter));
    assert!(std::ptr::eq(&raw const styles[HeadingLevel::Subparagraph], &raw const styles.subparagraph));
  }
}
