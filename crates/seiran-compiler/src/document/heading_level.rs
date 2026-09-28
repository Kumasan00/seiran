//! 見出しのレベル [`HeadingLevel`]。

use serde::Deserialize;

/// `\part` から `\subparagraph` までの見出しレベル
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HeadingLevel {
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
  /// 見出しレベルの総数（`Part` から `Subparagraph` までの 6 つ）
  pub(crate) const COUNT: usize = 6;

  /// 数値インデックスを返す（0=Part, 5=Subparagraph）
  #[must_use]
  pub(crate) fn depth(self) -> u8 { return self as u8; }

  /// コマンド名を返す
  #[must_use]
  pub(crate) fn command_name(self) -> &'static str {
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
mod tests {
  use super::HeadingLevel;

  #[test]
  fn heading_level_command_name() {
    assert_eq!(HeadingLevel::Part.command_name(), "part");
    assert_eq!(HeadingLevel::Chapter.command_name(), "chapter");
    assert_eq!(HeadingLevel::Section.command_name(), "section");
    assert_eq!(HeadingLevel::Subsection.command_name(), "subsection");
    assert_eq!(HeadingLevel::Paragraph.command_name(), "paragraph");
    assert_eq!(HeadingLevel::Subparagraph.command_name(), "subparagraph");
  }
}
