//! 見出しのレベル [`HeadingLevel`]。

use serde::Deserialize;

/// 見出しコマンドの種類（`\part` から `\subparagraph` までと `\appendix`）
///
/// 付録は章と同じ深さの見出しなので、深さ（[`HeadingLevel::depth`]）の同じ variant が 2 つある。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HeadingLevel {
  /// `\part` — 部（最上位の区分）
  Part,
  /// `\chapter` — 章
  Chapter,
  /// `\appendix` — 付録（章と同じ深さで、専用のカウンタで採番する）
  Appendix,
  /// `\section` — 節
  Section,
  /// `\subsection` — 小節
  Subsection,
  /// `\paragraph` — 段落見出し
  Paragraph,
  /// `\subparagraph` — 小段落見出し
  Subparagraph,
}

impl HeadingLevel {
  /// 見出しの深さの段数（0 = 部 〜 5 = 小段落）
  pub(crate) const DEPTHS: usize = 6;

  /// 深さを返す（0 = 部、1 = 章と付録、5 = 小段落）
  #[must_use]
  pub(crate) fn depth(self) -> u8 {
    return match self {
      HeadingLevel::Part => 0,
      HeadingLevel::Chapter | HeadingLevel::Appendix => 1,
      HeadingLevel::Section => 2,
      HeadingLevel::Subsection => 3,
      HeadingLevel::Paragraph => 4,
      HeadingLevel::Subparagraph => 5,
    };
  }

  /// コマンド名を返す
  #[must_use]
  fn command_name(self) -> &'static str {
    return match self {
      HeadingLevel::Part => "part",
      HeadingLevel::Chapter => "chapter",
      HeadingLevel::Appendix => "appendix",
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
    assert_eq!(HeadingLevel::Appendix.command_name(), "appendix");
  }

  #[test]
  fn appendix_shares_chapter_depth() {
    assert_eq!(HeadingLevel::Appendix.depth(), HeadingLevel::Chapter.depth());
  }
}
