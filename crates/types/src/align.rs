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

impl Align {
  /// 利用可能幅 `available` の中に幅 `content_width` の内容を置くときの水平オフセット（pt）。
  ///
  /// [`Align::Left`] は 0、[`Align::Center`] は中央、[`Align::Right`] は右端に寄せる。
  /// 内容が利用可能幅を超える場合は 0 にクランプし、左端より左へはみ出さない。
  /// 段落行・画像・罫線・表のいずれもこの 1 関数で揃えオフセットを算出する。
  #[must_use]
  pub fn offset(self, available: f32, content_width: f32) -> f32 {
    return match self {
      Align::Left => 0.0,
      Align::Center => ((available - content_width) / 2.0).max(0.0),
      Align::Right => (available - content_width).max(0.0),
    };
  }
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
  fn offset_left_is_always_zero() {
    // Arrange / Act / Assert — 左揃えは常に 0
    assert!((Align::Left.offset(100.0, 30.0) - 0.0).abs() < f32::EPSILON);
  }

  #[test]
  fn offset_center_is_half_remaining() {
    // Arrange / Act / Assert — 中央は余白の半分
    assert!((Align::Center.offset(100.0, 30.0) - 35.0).abs() < f32::EPSILON);
  }

  #[test]
  fn offset_right_is_full_remaining() {
    // Arrange / Act / Assert — 右は余白いっぱい
    assert!((Align::Right.offset(100.0, 30.0) - 70.0).abs() < f32::EPSILON);
  }

  #[test]
  fn offset_clamps_to_zero_when_content_overflows() {
    // Arrange / Act / Assert — 内容が利用可能幅を超えたら 0 にクランプ
    assert!((Align::Center.offset(30.0, 50.0) - 0.0).abs() < f32::EPSILON);
    assert!((Align::Right.offset(30.0, 50.0) - 0.0).abs() < f32::EPSILON);
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
