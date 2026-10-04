//! 水平方向の揃え [`Align`]。

use crate::{document::TextAlignment, length::Length};

/// ブロック（画像・表・数式）と数式セルの水平方向の揃え。段落の行は [`TextAlignment`] から変換して使う。
///
/// 揃えは確定した内容を利用可能幅の中で水平にシフトするだけ。内容が利用可能幅を超える場合のシフト量は
/// 0 にクランプされる（左端より左へはみ出さない）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum Align {
  /// 左揃え（ragged-right）
  Left,
  /// 中央揃え
  Center,
  /// 右揃え（ragged-left）
  Right,
}

impl Align {
  /// 利用可能幅 `available` の中に幅 `content_width` の内容を置くときの水平オフセット。
  #[must_use]
  pub(crate) fn offset(self, available: Length, content_width: Length) -> Length {
    return match self {
      Align::Left => Length::ZERO,
      Align::Center => ((available - content_width) / 2.0f32).max(Length::ZERO),
      Align::Right => (available - content_width).max(Length::ZERO),
    };
  }
}

/// 段落の揃えから、確定した行を寄せる向きを取り出す（両端揃えの行は左端から組む）。
impl From<TextAlignment> for Align {
  fn from(alignment: TextAlignment) -> Self {
    return match alignment {
      TextAlignment::Justify | TextAlignment::Left => Align::Left,
      TextAlignment::Center => Align::Center,
      TextAlignment::Right => Align::Right,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::Align;
  use crate::{document::TextAlignment, length::Length};

  #[test]
  fn offset_left_is_always_zero() {
    assert_eq!(Align::Left.offset(Length::pt(100.0), Length::pt(30.0)), Length::ZERO);
  }

  #[test]
  fn offset_center_is_half_remaining() {
    assert_eq!(Align::Center.offset(Length::pt(100.0), Length::pt(30.0)), Length::pt(35.0));
  }

  #[test]
  fn offset_right_is_full_remaining() {
    assert_eq!(Align::Right.offset(Length::pt(100.0), Length::pt(30.0)), Length::pt(70.0));
  }

  #[test]
  fn offset_clamps_to_zero_when_content_overflows() {
    assert_eq!(Align::Center.offset(Length::pt(30.0), Length::pt(50.0)), Length::ZERO);
    assert_eq!(Align::Right.offset(Length::pt(30.0), Length::pt(50.0)), Length::ZERO);
  }

  #[test]
  fn from_text_alignment_starts_justified_lines_at_left() {
    assert_eq!(Align::from(TextAlignment::Justify), Align::Left);
    assert_eq!(Align::from(TextAlignment::Left), Align::Left);
    assert_eq!(Align::from(TextAlignment::Center), Align::Center);
    assert_eq!(Align::from(TextAlignment::Right), Align::Right);
  }
}
