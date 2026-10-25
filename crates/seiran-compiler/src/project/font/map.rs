//! 全フォント種別に対応する値を 1 つずつ持つ表 [`FontMap`]。

use std::{array, ops::Index};

use strum::VariantArray;

use crate::project::font::FontType;

/// フォント種別の数（[`FontMap`] のスロット数）
const SLOTS: usize = FontType::VARIANTS.len();

// `FontMap` は判別子をそのまま配列の添字に使うので、ここが破れると別の種別の値を返してしまう。
const _: () = {
  let mut index = 0;
  while index < SLOTS {
    assert!(FontType::VARIANTS[index] as usize == index, "FontType::VARIANTS は宣言順に並んでいなければならない");
    index += 1;
  }
};

/// 全フォント種別 ([`FontType`]) に対応する値を 1 つずつ持つ読み取り専用の表
///
/// 値は `[T; 19]` に [`FontType`] の宣言順で並ぶので、全種別が揃っていることは型が保証する
/// （欠けた表・余った表は構築できない）。構築後に変更する経路は無く、種別での参照
/// （`map[font_type]`）と宣言順の消費だけを提供する。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FontMap<T> {
  /// [`FontType`] の宣言順に並んだ種別ごとの値
  values: [T; SLOTS],
}

impl<T> FontMap<T> {
  /// 各種別の値を `value_of` で作る（[`FontType`] の宣言順に 1 回ずつ呼ぶ）。
  pub(crate) fn from_fn(mut value_of: impl FnMut(FontType) -> T) -> Self {
    return Self {
      values: array::from_fn(|index| return value_of(FontType::VARIANTS[index])),
    };
  }

  /// 全種別の値を [`FontType`] の宣言順に消費する。
  pub(super) fn into_values(self) -> impl Iterator<Item = T> { return self.values.into_iter(); }
}

impl<T> Index<FontType> for FontMap<T> {
  type Output = T;

  // 添字は判別子。module 先頭の const 検査が `FontType::VARIANTS` と判別子の順序の一致を保証する。
  fn index(&self, font_type: FontType) -> &T { return &self.values[font_type as usize]; }
}

#[cfg(test)]
mod tests {
  use strum::VariantArray;

  use super::FontMap;
  use crate::project::font::FontType;

  #[test]
  fn from_fn_stores_each_value_under_its_font_type() {
    let map = FontMap::from_fn(|font_type| return font_type.as_toml_key());

    for &font_type in FontType::VARIANTS {
      assert_eq!(map[font_type], font_type.as_toml_key());
    }
  }

  #[test]
  fn into_values_yields_values_in_font_type_order() {
    let map = FontMap::from_fn(|font_type| return font_type);

    let values: Vec<FontType> = map.into_values().collect();

    assert_eq!(values, FontType::VARIANTS.to_vec());
  }
}
