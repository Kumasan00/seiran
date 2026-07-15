//! 表（table 環境）の列指定に関する共通型
//!
//! `frontend`（`columns=` / `widths=` の解析）、`DocNode` / `lowering`（IR・レイアウトノード）、
//! `pdf_gen`（列幅解決・描画）の全段で共有されるため契約クレートである本クレートに置く。

use crate::Length;

/// 表の 1 列の定義（揃え + 幅指定）
#[derive(Debug, Clone, Copy)]
pub struct TableColumn {
  /// セル内容の揃え方向
  pub align: ColumnAlign,
  /// 列幅の指定方法
  pub width: ColumnWidth,
}

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
/// 実際の幅解決（自然幅の実測・残余分配）は本クレートの [`resolve_column_widths`] で行われる（`typeset::breaking` が呼ぶ）。
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
  use super::{ColumnAlign, ColumnWidth, TableColumn};
  use crate::Length;

  #[test]
  fn column_align_default_is_left() {
    // #[default] が Left であることを確認する
    assert_eq!(ColumnAlign::default(), ColumnAlign::Left);
  }

  #[test]
  fn column_width_default_is_auto() {
    // #[default] が Auto であることを確認する
    assert_eq!(ColumnWidth::default(), ColumnWidth::Auto);
  }

  #[test]
  fn column_width_equality_by_variant_and_value() {
    // 同一変種・同一値は等しく、値や変種が違えば等しくない
    assert_eq!(ColumnWidth::Fixed(Length::pt(5.0)), ColumnWidth::Fixed(Length::pt(5.0)));
    assert_ne!(ColumnWidth::Fixed(Length::pt(5.0)), ColumnWidth::Fixed(Length::pt(6.0)));
    assert_ne!(ColumnWidth::Ratio(0.3), ColumnWidth::Ratio(0.5));
    assert_ne!(ColumnWidth::Auto, ColumnWidth::Flex);
  }

  #[test]
  fn table_column_holds_align_and_width() {
    // TableColumn は揃えと幅指定をそのまま保持する
    let col = TableColumn {
      align: ColumnAlign::Right,
      width: ColumnWidth::Ratio(0.25),
    };
    assert_eq!(col.align, ColumnAlign::Right);
    assert_eq!(col.width, ColumnWidth::Ratio(0.25));
  }

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
