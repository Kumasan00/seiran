//! 見出しのレベル [`HeadingLevel`]。

use serde::{Deserialize, Serialize};

/// `\part` から `\subparagraph` までの見出しレベル
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadingLevel {
  /// `\part` — 部（最上位の区分）
  Part = 0,
  /// `\chapter` — 章
  Chapter = 1,
  /// `\section` — 節
  Section = 2,
  /// `\subsection` — 小節
  Subsection = 3,
  /// `\paragraph` — 段落見出し
  Paragraph = 4,
  /// `\subparagraph` — 小段落見出し
  Subparagraph = 5,
}

impl HeadingLevel {
  /// 6 つのレベルすべてを宣言順で並べた配列
  #[allow(dead_code)]
  pub const ALL: [HeadingLevel; 6] = [
    HeadingLevel::Part,
    HeadingLevel::Chapter,
    HeadingLevel::Section,
    HeadingLevel::Subsection,
    HeadingLevel::Paragraph,
    HeadingLevel::Subparagraph,
  ];
  /// `HeadingLevel::ALL` の要素数
  pub const COUNT: usize = 6;

  /// 数値インデックスを返す（0=Part, 5=Subparagraph）
  #[must_use]
  pub fn depth(self) -> u8 { return self as u8; }

  /// コマンド名からレベルを取得する
  #[must_use]
  #[allow(dead_code)]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "part" => Some(HeadingLevel::Part),
      "chapter" => Some(HeadingLevel::Chapter),
      "section" => Some(HeadingLevel::Section),
      "subsection" => Some(HeadingLevel::Subsection),
      "paragraph" => Some(HeadingLevel::Paragraph),
      "subparagraph" => Some(HeadingLevel::Subparagraph),
      _ => None,
    };
  }

  /// コマンド名を返す
  #[must_use]
  pub fn command_name(self) -> &'static str {
    return match self {
      HeadingLevel::Part => "part",
      HeadingLevel::Chapter => "chapter",
      HeadingLevel::Section => "section",
      HeadingLevel::Subsection => "subsection",
      HeadingLevel::Paragraph => "paragraph",
      HeadingLevel::Subparagraph => "subparagraph",
    };
  }
}

impl std::fmt::Display for HeadingLevel {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.command_name()); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::HeadingLevel;

  #[test]
  fn heading_level_depth_returns_correct_values() {
    assert_eq!(HeadingLevel::Part.depth(), 0);
    assert_eq!(HeadingLevel::Chapter.depth(), 1);
    assert_eq!(HeadingLevel::Section.depth(), 2);
    assert_eq!(HeadingLevel::Subsection.depth(), 3);
    assert_eq!(HeadingLevel::Paragraph.depth(), 4);
    assert_eq!(HeadingLevel::Subparagraph.depth(), 5);
  }

  #[test]
  fn heading_level_ordering() {
    assert!(HeadingLevel::Part < HeadingLevel::Chapter);
    assert!(HeadingLevel::Chapter < HeadingLevel::Section);
    assert!(HeadingLevel::Section < HeadingLevel::Subsection);
    assert!(HeadingLevel::Subsection < HeadingLevel::Paragraph);
    assert!(HeadingLevel::Paragraph < HeadingLevel::Subparagraph);
  }

  #[test]
  fn heading_level_from_command_name() {
    assert_eq!(HeadingLevel::from_command_name("part"), Some(HeadingLevel::Part));
    assert_eq!(HeadingLevel::from_command_name("chapter"), Some(HeadingLevel::Chapter));
    assert_eq!(HeadingLevel::from_command_name("section"), Some(HeadingLevel::Section));
    assert_eq!(HeadingLevel::from_command_name("subsection"), Some(HeadingLevel::Subsection));
    assert_eq!(HeadingLevel::from_command_name("paragraph"), Some(HeadingLevel::Paragraph));
    assert_eq!(HeadingLevel::from_command_name("subparagraph"), Some(HeadingLevel::Subparagraph));
    assert_eq!(HeadingLevel::from_command_name("unknown"), None);
  }

  #[test]
  fn heading_level_command_name() {
    assert_eq!(HeadingLevel::Part.command_name(), "part");
    assert_eq!(HeadingLevel::Chapter.command_name(), "chapter");
    assert_eq!(HeadingLevel::Section.command_name(), "section");
    assert_eq!(HeadingLevel::Subsection.command_name(), "subsection");
    assert_eq!(HeadingLevel::Paragraph.command_name(), "paragraph");
    assert_eq!(HeadingLevel::Subparagraph.command_name(), "subparagraph");
  }

  #[test]
  fn heading_level_display() {
    assert_eq!(format!("{}", HeadingLevel::Section), "section");
    assert_eq!(format!("{}", HeadingLevel::Part), "part");
  }
}
