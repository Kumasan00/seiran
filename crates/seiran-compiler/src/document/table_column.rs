//! 著者が `columns=` / `widths=` に書く表の列指定語彙。
//!
//! どちらも HIR（`HirNodeKind::Table`）に直接現れる authored な語彙なので `document` が持つ。
//! 2 つを列ごとに束ねた組版入力 `TableColumn` は `typeset::boxes` の所有（#334）。

use std::str::FromStr;

use thiserror::Error;

use crate::length::Length;

/// 列内のセル内容の揃え方向
///
/// 環境任意引数 `columns="left center right"` の各トークンに対応する。
/// LaTeX の `l/c/r` 略記は採用せずフルスペルのみを受理する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ColumnAlign {
  /// 左揃え（既定）
  #[default]
  Left,
  /// 中央揃え
  Center,
  /// 右揃え
  Right,
}

/// [`ColumnAlign`] の `FromStr` が受理しないキーワードを渡されたときのエラー。
#[derive(Debug, Error)]
#[error("列の揃えは left / center / right のいずれかである必要があります")]
pub(crate) struct ParseColumnAlignError;

impl FromStr for ColumnAlign {
  type Err = ParseColumnAlignError;

  /// `columns=` のトークン（フルスペル）から揃え方向を解決する
  ///
  /// `l` / `c` / `r` の略記は受理しない。前後の空白も落とさない（呼び出し側が
  /// `split_whitespace` で切り出したトークンを渡す）。
  fn from_str(keyword: &str) -> Result<Self, Self::Err> {
    return match keyword {
      "left" => Ok(ColumnAlign::Left),
      "center" => Ok(ColumnAlign::Center),
      "right" => Ok(ColumnAlign::Right),
      _ => Err(ParseColumnAlignError),
    };
  }
}

/// 列幅の指定方法
///
/// 環境任意引数 `widths="auto 5cm 0.3 *"` の各トークンに対応する。
/// 実際の幅解決（自然幅の実測・残余分配）は本クレートの [`resolve_column_widths`] で行われる（`typeset::breaking` が呼ぶ）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum ColumnWidth {
  /// 内容の自然幅に合わせる（既定）
  #[default]
  Auto,
  /// 固定長（`5cm` / `30mm` 等）
  Fixed(Length),
  /// 本文幅に対する比率（`0.3` 等、0 より大きく 1 以下）
  Ratio(f32),
  /// 残り幅を等分するフレックス指定（`*`）
  Flex,
}

#[cfg(test)]
mod tests {
  use super::{ColumnAlign, ColumnWidth};
  use crate::length::Length;

  #[test]
  fn column_align_default_is_left() {
    // Arrange / Act / Assert
    assert_eq!(ColumnAlign::default(), ColumnAlign::Left);
  }

  #[test]
  fn column_width_default_is_auto() {
    // Arrange / Act / Assert
    assert_eq!(ColumnWidth::default(), ColumnWidth::Auto);
  }

  #[test]
  fn column_width_equality_by_variant_and_value() {
    // Arrange / Act / Assert
    assert_eq!(ColumnWidth::Fixed(Length::pt(5.0)), ColumnWidth::Fixed(Length::pt(5.0)));
    assert_ne!(ColumnWidth::Fixed(Length::pt(5.0)), ColumnWidth::Fixed(Length::pt(6.0)));
    assert_ne!(ColumnWidth::Ratio(0.3), ColumnWidth::Ratio(0.5));
    assert_ne!(ColumnWidth::Auto, ColumnWidth::Flex);
  }

  #[test]
  fn from_str_resolves_full_spellings() {
    // Arrange / Act / Assert
    assert_eq!("left".parse::<ColumnAlign>().ok(), Some(ColumnAlign::Left));
    assert_eq!("center".parse::<ColumnAlign>().ok(), Some(ColumnAlign::Center));
    assert_eq!("right".parse::<ColumnAlign>().ok(), Some(ColumnAlign::Right));
  }

  #[test]
  fn from_str_rejects_abbreviations_and_unknown() {
    // Arrange / Act / Assert
    assert!("l".parse::<ColumnAlign>().is_err());
    assert!("c".parse::<ColumnAlign>().is_err());
    assert!("r".parse::<ColumnAlign>().is_err());
    assert!("justify".parse::<ColumnAlign>().is_err());
    assert!("".parse::<ColumnAlign>().is_err());
  }
}
