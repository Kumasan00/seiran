//! 水平方向の揃え [`Align`]。
//!
//! 段落・行を本文幅のどこに寄せるかを表す。既定は左揃え（ragged-right）で、
//! 本文の自動折り返しはこれを前提にしている。中央・右揃えはタイトルページの
//! 中央寄せなど、確定済みの行を本文幅の中で水平にずらす用途で使う。
//!
//! `hlist::Block::Paragraph` と `lowering::LayoutNode::VBox` の双方が保持し、
//! lowering → layout → `hlist::break_pages` を透過して確定行のオフセットに反映される。

use serde::{Deserialize, Serialize};

/// 段落・行の水平方向の揃え。
///
/// 揃えは行折り返しには影響せず（折り返しは常に利用可能幅で行う）、確定した各行を
/// 利用可能幅の中で水平にシフトするだけ。行が利用可能幅を超える場合のシフト量は
/// 0 にクランプされる（行頭が本文左端より左へはみ出さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
  /// 左揃え（ragged-right、既定）
  #[default]
  Left,
  /// 中央揃え
  Center,
  /// 右揃え（ragged-left）
  Right,
}

#[cfg(test)]
mod tests {
  use super::Align;

  #[test]
  fn default_is_left() {
    // Arrange / Act / Assert — 既定は左揃え
    assert_eq!(Align::default(), Align::Left);
  }

  #[test]
  fn deserializes_snake_case() {
    // Arrange
    #[derive(serde::Deserialize)]
    struct Wrapper {
      align: Align,
    }

    // Act
    let w: Wrapper = toml::from_str("align = \"center\"").unwrap();

    // Assert
    assert_eq!(w.align, Align::Center);
  }
}
