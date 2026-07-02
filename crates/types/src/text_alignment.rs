//! 本文段落の行末処理 [`TextAlignment`]。
//!
//! 行分割で確定した各行の余り幅をどう扱うかを表す。両端揃え（justify）は
//! 余り幅を行内の伸縮点（欧文単語間スペース等）へ比例配分して行末を版面右端に
//! 揃え、左揃え（ragged-right）は自然幅のまま並べて行末を成り行きに任せる。
//!
//! `style.toml` の `[text] alignment` が値の出どころで、`read_style` →
//! `hlist::break_pages` → `hlist::break_lines` へ透過する。段落単位の水平揃え
//! （[`crate::Align`] の中央・右寄せ）とは独立で、中央・右寄せ段落には適用されない。

use serde::{Deserialize, Serialize};

/// 本文段落の行末処理（両端揃え / 左揃え）。
///
/// 行分割の分割点選択には影響せず、確定した行内の伸縮点（`stretch` / `shrink`
/// 能力を持つ glue）の幅だけを変える。段落最終行・強制改行直前の行は
/// 両端揃えでも伸縮しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
  /// 両端揃え（既定）。行の余り幅を伸縮点へ比例配分して行末を版面右端に揃える
  #[default]
  Justify,
  /// 左揃え（ragged-right）。伸縮点を使わず自然幅のまま並べる
  RaggedRight,
}

#[cfg(test)]
mod tests {
  use super::TextAlignment;

  #[test]
  fn default_is_justify() {
    // Arrange / Act / Assert — 既定は両端揃え
    assert_eq!(TextAlignment::default(), TextAlignment::Justify);
  }

  #[test]
  fn deserializes_snake_case() {
    // Arrange
    #[derive(serde::Deserialize)]
    struct Wrapper {
      alignment: TextAlignment,
    }

    // Act
    let justify: Wrapper = toml::from_str("alignment = \"justify\"").unwrap();
    let ragged: Wrapper = toml::from_str("alignment = \"ragged_right\"").unwrap();

    // Assert
    assert_eq!(justify.alignment, TextAlignment::Justify);
    assert_eq!(ragged.alignment, TextAlignment::RaggedRight);
  }
}
