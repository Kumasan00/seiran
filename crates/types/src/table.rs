//! 表（table 環境)の列指定に関する共通型
//!
//! `parser`（評価時の `columns=` / `widths=` 解析結果）、`lowering`（`LayoutNode::Table`）、
//! `layout` / `pdf_gen`（列幅解決と描画）の全段で共有されるため `types` クレートに置く。

use crate::Length;

/// 列内のセル内容の揃え方向
///
/// 環境任意引数 `columns="left center right"` の各トークンに対応する。
/// LaTeX の `l/c/r` 略記は採用せずフルスペルのみを受理する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColumnAlign {
  /// 左揃え（既定）
  #[default]
  Left,
  /// 中央揃え
  Center,
  /// 右揃え
  Right,
}

impl ColumnAlign {
  /// `columns=` のトークン（フルスペル）から揃え方向を解決する
  ///
  /// 未知のトークンは `None` を返す（`l` / `c` / `r` 略記も不可）。
  #[must_use]
  pub fn from_keyword(keyword: &str) -> Option<Self> {
    return match keyword {
      "left" => Some(ColumnAlign::Left),
      "center" => Some(ColumnAlign::Center),
      "right" => Some(ColumnAlign::Right),
      _ => None,
    };
  }
}

/// 列幅の指定方法
///
/// 環境任意引数 `widths="auto 5cm 0.3 *"` の各トークンに対応する。
/// 実際の幅解決（自然幅の実測・残余分配）は `pdf_gen` 段で行われる。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ColumnWidth {
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
  use super::ColumnAlign;

  #[test]
  fn from_keyword_resolves_full_spellings() {
    assert_eq!(ColumnAlign::from_keyword("left"), Some(ColumnAlign::Left));
    assert_eq!(ColumnAlign::from_keyword("center"), Some(ColumnAlign::Center));
    assert_eq!(ColumnAlign::from_keyword("right"), Some(ColumnAlign::Right));
  }

  #[test]
  fn from_keyword_rejects_abbreviations_and_unknown() {
    // LaTeX 風の 1 文字略記は採用しない
    assert_eq!(ColumnAlign::from_keyword("l"), None);
    assert_eq!(ColumnAlign::from_keyword("c"), None);
    assert_eq!(ColumnAlign::from_keyword("r"), None);
    assert_eq!(ColumnAlign::from_keyword("justify"), None);
    assert_eq!(ColumnAlign::from_keyword(""), None);
  }
}
