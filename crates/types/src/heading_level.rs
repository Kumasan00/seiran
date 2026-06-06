//! 見出しのレベル [`HeadingLevel`] の定義
//!
//! `\part` 〜 `\subparagraph` の 6 段階の論理レベルを表す列挙型と、
//! コマンド名との相互変換を提供します。見出し固有のフォント・余白・番号書式は
//! `read_style::HeadingStyle` 側で保持し、ここではレベルそのものに関する
//! 基本変換のみを提供します。

use serde::{Deserialize, Serialize};

/// 見出しのレベル
///
/// LaTeX の見出しコマンドに対応する 6 段階の論理レベル。
/// `\part` を最上位とし、`\subparagraph` を最下位とする。
/// 見出し固有のフォント・余白・番号書式は `read_style::HeadingStyle` で持たせ、
/// このクレートではレベルの enum と基本変換のみを提供する。
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
