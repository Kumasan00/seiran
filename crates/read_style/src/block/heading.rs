//! 見出し要素（part / chapter / section …）のスタイル設定型。
//!
//! TOML 上の `[heading]` テーブルは 3 レイヤーで解釈する:
//!
//! 1. [`default_for_level`] で得る Rust 既定（Part: 40pt 改ページあり、Section: 20pt …）
//! 2. `[heading]` 直下のスカラー値（全レベル共通の base 上書き）
//! 3. `[heading.<level>]` テーブル（レベル別の差分上書き）
//!
//! このマージは [`deserialize_per_level`] が `HeadingTable` 経由で実行する。

use garde::Validate;
use serde::{Deserialize, Deserializer, Serialize};
use types::{FontKind, HeadingLevel};

use crate::common::{
  length::{Length, non_negative, positive},
  per_level::PerLevel,
};

/// 見出し要素のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct HeadingStyle {
  /// 見出しの書式テンプレート。`{number}` と `{title}` を含めることができる
  #[garde(length(chars, min = 1))]
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
  /// `HeadingLevel::Section` 相当の汎用デフォルト（`{number} {title}` / 20pt / 太字セリフ）。
  ///
  /// レベルごとの差分は [`default_for_level`] が与える。デシリアライズ時の `#[serde(default)]`
  /// で部分指定をサポートするために必要。
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

/// 見出しレベル全 6 つに対応する [`PerLevel<HeadingStyle>`] のデフォルトを生成する。
///
/// `HeadingStyle::default` で共通項を埋めつつ、レベル固有の値（フォントサイズ・テンプレ・改頁）だけ
/// このテーブルで上書きする。`Style::default()` の手書き重複を 1 箇所に集約する。
#[must_use]
pub fn default_per_level() -> PerLevel<HeadingStyle> { return PerLevel::from_fn(default_for_level); }

/// [`HeadingStyle`] の各フィールドを `Option<_>` で覆った差分指定型。
///
/// `[heading]` 直下の base 指定、`[heading.<level>]` のレベル別差分の両方に使う。
/// 未指定（`None`）フィールドは下層の既定値を残す。
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

/// `[heading]` テーブル全体の TOML スキーマ。
///
/// テーブル直下のスカラーフィールドが base（全レベル共通の上書き）として機能し、
/// `part` / `chapter` / … サブテーブルが各レベルの差分を担う。
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct HeadingTable {
  /// base: 見出しの書式テンプレート
  pub format: Option<String>,
  /// base: フォントサイズ
  pub font_size: Option<Length>,
  /// base: 下余白
  pub bottom_margin: Option<Length>,
  /// base: 改ページ前
  pub page_break_before: Option<bool>,
  /// base: 改ページ後
  pub page_break_after: Option<bool>,
  /// base: フォント種別
  pub font_kind: Option<FontKind>,
  /// `[heading.part]` 差分
  pub part: Option<HeadingStyleOverride>,
  /// `[heading.chapter]` 差分
  pub chapter: Option<HeadingStyleOverride>,
  /// `[heading.section]` 差分
  pub section: Option<HeadingStyleOverride>,
  /// `[heading.subsection]` 差分
  pub subsection: Option<HeadingStyleOverride>,
  /// `[heading.paragraph]` 差分
  pub paragraph: Option<HeadingStyleOverride>,
  /// `[heading.subparagraph]` 差分
  pub subparagraph: Option<HeadingStyleOverride>,
}

impl HeadingTable {
  /// 3 レイヤーマージ（Rust 既定 → base → レベル別差分）を適用して [`PerLevel<HeadingStyle>`] を構築する。
  fn into_per_level(self) -> PerLevel<HeadingStyle> {
    let HeadingTable {
      format,
      font_size,
      bottom_margin,
      page_break_before,
      page_break_after,
      font_kind,
      part,
      chapter,
      section,
      subsection,
      paragraph,
      subparagraph,
    } = self;
    let base = HeadingStyleOverride {
      format,
      font_size,
      bottom_margin,
      page_break_before,
      page_break_after,
      font_kind,
    };
    let build = |level: HeadingLevel, over: Option<&HeadingStyleOverride>| -> HeadingStyle {
      let mut style = default_for_level(level);
      base.apply(&mut style);
      if let Some(over) = over {
        over.apply(&mut style);
      }
      return style;
    };
    return PerLevel::new([
      build(HeadingLevel::Part, part.as_ref()),
      build(HeadingLevel::Chapter, chapter.as_ref()),
      build(HeadingLevel::Section, section.as_ref()),
      build(HeadingLevel::Subsection, subsection.as_ref()),
      build(HeadingLevel::Paragraph, paragraph.as_ref()),
      build(HeadingLevel::Subparagraph, subparagraph.as_ref()),
    ]);
  }
}

/// `Style::heading` 用のカスタムデシリアライザ。
///
/// `[heading]` テーブルを [`HeadingTable`] で受け、3 レイヤーマージ（Rust 既定 → base → レベル別差分）
/// を適用して [`PerLevel<HeadingStyle>`] を返す。
///
/// `#[serde(deserialize_with = "deserialize_per_level")]` で `Style::heading` に取り付けて使う。
///
/// # Errors
///
/// - TOML がテーブルとしてデシリアライズできない場合
/// - 未知のキーやレベル名が含まれる場合（`#[serde(deny_unknown_fields)]` 経由）
pub fn deserialize_per_level<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PerLevel<HeadingStyle>, D::Error> {
  let table = HeadingTable::deserialize(deserializer)?;
  return Ok(table.into_per_level());
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
  use types::{FontKind, HeadingLevel};

  use super::{HeadingStyle, default_for_level, default_per_level};
  use crate::common::length::Length;

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

  #[test]
  fn heading_table_base_propagates_to_all_levels() {
    // Arrange: base で font_kind だけ差し替え
    let toml = "font_kind = \"sans_serif_bold\"\n";

    // Act
    let table: super::HeadingTable = toml::from_str(toml).unwrap();
    let per_level = table.into_per_level();

    // Assert: 全レベルが base 値を継承し、レベル固有の font_size は維持
    for level in HeadingLevel::ALL {
      assert_eq!(per_level[level].font_kind, FontKind::SansSerifBold);
    }
    assert!((per_level[HeadingLevel::Part].font_size.to_pt() - 40.0).abs() < f32::EPSILON);
    assert!((per_level[HeadingLevel::Section].font_size.to_pt() - 20.0).abs() < f32::EPSILON);
  }

  #[test]
  fn heading_table_level_override_wins_over_base() {
    // Arrange: base で全レベル 30pt、Section だけ 18pt
    let toml = "
font_size = \"30pt\"

[section]
font_size = \"18pt\"
";

    // Act
    let table: super::HeadingTable = toml::from_str(toml).unwrap();
    let per_level = table.into_per_level();

    // Assert
    assert!((per_level[HeadingLevel::Part].font_size.to_pt() - 30.0).abs() < f32::EPSILON);
    assert!((per_level[HeadingLevel::Chapter].font_size.to_pt() - 30.0).abs() < f32::EPSILON);
    assert!((per_level[HeadingLevel::Section].font_size.to_pt() - 18.0).abs() < f32::EPSILON);
    assert!((per_level[HeadingLevel::Subparagraph].font_size.to_pt() - 30.0).abs() < f32::EPSILON);
  }

  #[test]
  fn heading_table_rejects_unknown_level_key() {
    // Arrange
    let toml = "
[unknown_level]
font_size = \"12pt\"
";

    // Act
    let result: Result<super::HeadingTable, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "未知のレベル名は拒否されるべき: {result:?}");
  }

  #[test]
  fn heading_table_partial_level_keeps_other_defaults() {
    // Arrange: Section の format のみ上書き
    let toml = "
[section]
format = \"§ {number} {title}\"
";

    // Act
    let table: super::HeadingTable = toml::from_str(toml).unwrap();
    let per_level = table.into_per_level();

    // Assert: Section の font_size はレベル既定の 20pt を維持
    assert_eq!(per_level[HeadingLevel::Section].format, "§ {number} {title}");
    assert!((per_level[HeadingLevel::Section].font_size.to_pt() - 20.0).abs() < f32::EPSILON);
    // 他レベルは default_for_level のまま
    assert!(per_level[HeadingLevel::Part].page_break_after);
  }
}
