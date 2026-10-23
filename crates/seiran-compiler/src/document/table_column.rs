//! 著者が `columns=` / `widths=` に書く表の列指定語彙と、`\row[separator=...]` に書く行の区切りの語彙。

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

/// 表の行の上の境界に置く区切り（`\row[separator=...]`）
///
/// 区切りの無い行はキーを書かないことで表し、`none` のような値は設けない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowSeparator {
  /// 罫線（太さは style.toml の `[table].rule_thickness`、上下の空きは `rule_gap`）
  Rule,
  /// 空き（量は style.toml の `[table].row_gap`）
  Gap,
}

/// [`RowSeparator`] の `FromStr` が受理しない値を渡されたときのエラー。
#[derive(Debug, Error)]
#[error("区切りは rule / gap のいずれかである必要があります")]
pub(crate) struct ParseRowSeparatorError;

impl FromStr for RowSeparator {
  type Err = ParseRowSeparatorError;

  /// `\row[separator=...]` の値を区切りに変換する
  ///
  /// 受理する値は小文字の `rule` / `gap` の完全一致だけ。大文字を含む綴りは同じ値への第 2 の綴りになるので受理しない。
  /// 前後の空白は任意引数の値の取り出しが除く。
  fn from_str(value: &str) -> Result<Self, Self::Err> {
    return match value {
      "rule" => Ok(RowSeparator::Rule),
      "gap" => Ok(RowSeparator::Gap),
      _ => Err(ParseRowSeparatorError),
    };
  }
}
