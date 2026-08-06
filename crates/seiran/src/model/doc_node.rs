//! ブロックレベル要素の型定義
//!
//! 著者が書いた本文は HIR（`model::hir`）が持つため、ここに残る variant は
//! `citation::render` が書誌として組み立てる生成物の語彙に絞られる（#325）。

use crate::model::{CitationId, HeadingLevel, InlineNode, Span};

/// 引用の生成物（書誌）が使うブロック要素
///
/// セマンティック情報のみを保持し、フォントサイズや座標などの物理レイアウトは含まない。
#[derive(Debug, Clone, PartialEq)]
pub enum DocNode {
  /// 見出し（CSL 整形が合成する「References」見出し）
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 採番対象かどうか。`true` なら `lowering` 層が対応するカウンタを発番する。
    ///
    /// CSL 整形ステージ（`citation` クレート）が合成する「References」見出しは常に `false`。
    numbered: bool,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
    /// 参照ラベル。生成物の見出しは常に `None`
    label: Option<String>,
    /// ソース位置。生成物の見出しは常に `Span::DUMMY`
    span: Span,
  },

  /// 段落（インライン要素の集合。書誌の各エントリ本文）
  Paragraph(Vec<InlineNode>),

  /// 参考文献エントリに置くゼロサイズの参照アンカー
  Anchor(CitationId),
}

#[cfg(test)]
mod tests {
  use super::*;

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
