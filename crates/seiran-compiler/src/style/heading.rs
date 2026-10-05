//! 見出し要素（part / chapter / section …）のスタイル設定型。

use std::ops::Index;

use garde::Validate;
use serde::Deserialize;

use crate::{
  document::{HeadingLevel, TextAlignment, Typeface},
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
  /// 見出しテキストの書体
  pub typeface: Typeface,
  /// 見出し行の揃え。`None` は外側の縦リストの揃え（既定では `[text].alignment`）に従う。`section` の値は目次の題目行にも効く
  pub alignment: Option<TextAlignment>,
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
      typeface: Typeface::SerifBold,
      alignment: None,
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
  /// 見出しテキストの書体
  typeface: Option<Typeface>,
  /// 見出し行の揃え
  alignment: Option<TextAlignment>,
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
      typeface,
      alignment,
    } = self;
    return HeadingStyle {
      format: format.unwrap_or(base.format),
      font_size: font_size.unwrap_or(base.font_size),
      bottom_margin: bottom_margin.unwrap_or(base.bottom_margin),
      page_break_before: page_break_before.unwrap_or(base.page_break_before),
      page_break_after: page_break_after.unwrap_or(base.page_break_after),
      typeface: typeface.unwrap_or(base.typeface),
      alignment: alignment.or(base.alignment),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::{HeadingStyle, HeadingStyles};
  use crate::{
    document::{HeadingLevel, TextAlignment, Typeface},
    length::Length,
  };

  /// `HeadingStyles` を TOML から `[heading.<level>]` 配下に書く形でテストするための薄いラッパ。
  #[derive(Debug, serde::Deserialize)]
  struct HeadingWrapper {
    heading: HeadingStyles,
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
      assert_eq!(style.typeface, Typeface::SerifBold, "{level:?} の書体");
    }
  }

  #[test]
  fn heading_styles_rejects_unknown_level_key() {
    let toml = "
[heading.unknown_level]
font_size = \"12pt\"
";
    let result: Result<HeadingWrapper, _> = toml::from_str(toml);
    assert!(result.is_err(), "未知のレベル名は拒否されるべき: {result:?}");
  }

  #[test]
  fn heading_styles_rejects_base_scalar_keys() {
    let toml = "
[heading]
typeface = \"sans_serif_bold\"
";
    let result: Result<HeadingWrapper, _> = toml::from_str(toml);
    assert!(result.is_err(), "[heading] 直下のスカラー指定は拒否されるべき: {result:?}");
  }

  #[test]
  fn heading_styles_partial_level_keeps_other_defaults() {
    let toml = "
[heading.section]
format = \"§ {number} {title}\"
";

    let wrapper: HeadingWrapper = toml::from_str(toml).unwrap();
    let styles = wrapper.heading;

    assert_eq!(styles[HeadingLevel::Section].format.as_str(), "§ {number} {title}");
    assert!((styles[HeadingLevel::Section].font_size.to_pt() - 20.0).abs() < f32::EPSILON);
    assert!(styles[HeadingLevel::Part].page_break_after);
    assert_eq!(styles[HeadingLevel::Part].typeface, Typeface::SerifBold);
  }

  #[test]
  fn heading_styles_full_level_overrides_every_key() {
    // 1 レベルに 6 キー全部を書いた形（同型のフィールド同士の取り違えを検出する）。
    // chapter は改ページの既定が before = true / after = false なので、逆の値で取り違えも無視も落ちる
    let toml = "
[heading.chapter]
format = \"{title}\"
font_size = \"13pt\"
bottom_margin = \"4pt\"
page_break_before = false
page_break_after = true
typeface = \"sans_serif_bold\"
";

    let wrapper: HeadingWrapper = toml::from_str(toml).unwrap();
    let chapter = &wrapper.heading[HeadingLevel::Chapter];

    assert_eq!(chapter.format.as_str(), "{title}");
    assert_eq!(chapter.font_size, Length::pt(13.0));
    assert_eq!(chapter.bottom_margin, Length::pt(4.0));
    assert!(!chapter.page_break_before);
    assert!(chapter.page_break_after);
    assert_eq!(chapter.typeface, Typeface::SansSerifBold);
  }

  #[test]
  fn heading_alignment_is_unspecified_for_every_level_by_default() {
    let styles = HeadingStyles::default();
    let levels = [
      HeadingLevel::Part,
      HeadingLevel::Chapter,
      HeadingLevel::Section,
      HeadingLevel::Subsection,
      HeadingLevel::Paragraph,
      HeadingLevel::Subparagraph,
    ];

    for level in levels {
      assert_eq!(styles[level].alignment, None, "{level:?} の既定は未指定（外側の揃えに従う）");
    }
  }

  #[test]
  fn heading_alignment_overrides_only_the_given_level() {
    let wrapper: HeadingWrapper = toml::from_str("[heading.section]\nalignment = \"center\"\n").unwrap();

    assert_eq!(wrapper.heading.section.alignment, Some(TextAlignment::Center));
    assert_eq!(wrapper.heading.subsection.alignment, None);
  }
}
