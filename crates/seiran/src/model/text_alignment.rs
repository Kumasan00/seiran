//! 本文段落の行末処理 [`TextAlignment`]。

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
    // Arrange / Act / Assert
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
