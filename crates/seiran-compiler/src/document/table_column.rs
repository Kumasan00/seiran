//! 著者が `columns=` / `widths=` に書く表の列指定語彙。

use std::str::FromStr;

use thiserror::Error;

use crate::length::Length;

/// 列内のセル内容の揃え方向
///
/// 環境任意引数 `columns=left center right` の各トークンに対応する。
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
  /// 前後の空白は落とさない（呼び出し側が `split_whitespace` で切り出したトークンを渡す）。
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
/// 環境任意引数 `widths=auto 5cm 0.3 *` の各トークンに対応する。
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
