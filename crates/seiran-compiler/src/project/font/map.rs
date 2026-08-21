//! 全フォント種別に対応する値を保持する [`FontMap`]。

use std::collections::HashMap;

use crate::project::font::FontType;

/// 全フォント種別 ([`FontType`]) に対応する値を保持する汎用コンテナ
///
/// イテレーション時は [`FontType::ALL`] の順序で要素を返す。
///
/// # Examples
///
/// ```ignore
/// // `project::font` は非公開 module のため、この例は擬似コードとして提示するのみ（コンパイル・
/// // 実行はしない）。実際の検証は本ファイル末尾の `#[cfg(test)] mod tests` を参照（#307 の model
/// // crate 吸収で `FontMap` / `FontType` が crate 外から到達不能になり、rustdoc テストとしては
/// // 成立しなくなった。#352 で所有者が `project::font` に移った後も同じ）。
/// use crate::project::font::{FontMap, FontType};
///
/// let map = FontMap::from_all(FontType::ALL.iter().map(|ft| format!("{ft}")));
/// assert_eq!(map.get(FontType::Serif), "Serif");
/// assert_eq!(map.iter().count(), 19);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FontMap<T> {
  /// フォント種別ごとの値
  inner: HashMap<FontType, T>,
}

impl<T> FontMap<T> {
  /// [`FontType::ALL`] の順序に対応するイテレータから構築する
  ///
  /// # Panics
  ///
  /// イテレータの要素数が [`FontType::ALL`] の要素数と異なる場合にパニックする。
  pub fn from_all(values: impl IntoIterator<Item = T>) -> Self {
    let inner: HashMap<FontType, T> = FontType::ALL.into_iter().zip(values).collect();
    assert_eq!(inner.len(), FontType::ALL.len(), "FontMap: 要素数が FontType::ALL と一致しません");
    return Self { inner };
  }

  /// 指定されたフォント種別の値を返す
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  /// `from_all` で正しく構築されていれば発生しません。
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &T { return &self.inner[&font_type]; }

  /// 指定されたフォント種別の値を可変参照で返す
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  #[must_use]
  pub fn get_mut(&mut self, font_type: FontType) -> &mut T {
    return self.inner.get_mut(&font_type).expect("FontMap: 指定された FontType が見つかりません");
  }

  /// [`FontType::ALL`] の順序で反復する
  #[must_use]
  pub fn iter(&self) -> FontMapIter<'_, T> {
    return FontMapIter {
      inner: &self.inner,
      index: 0,
    };
  }

  /// [`FontType::ALL`] の順序で可変反復する
  pub fn iter_mut(&mut self) -> FontMapIterMut<'_, T> {
    return FontMapIterMut {
      inner: &mut self.inner,
      index: 0,
    };
  }
}

/// [`FontMap`] の不変イテレータ
///
/// [`FontType::ALL`] の順序で `(FontType, &T)` を返します。
pub struct FontMapIter<'a, T> {
  /// 走査対象
  inner: &'a HashMap<FontType, T>,
  /// 現在位置
  index: usize,
}

impl<'a, T> Iterator for FontMapIter<'a, T> {
  type Item = (FontType, &'a T);

  fn next(&mut self) -> Option<Self::Item> {
    if self.index >= FontType::ALL.len() {
      return None;
    }
    let font_type = FontType::ALL[self.index];
    let value = &self.inner[&font_type];
    self.index += 1;
    return Some((font_type, value));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl<T> ExactSizeIterator for FontMapIter<'_, T> {}

/// [`FontMap`] の可変イテレータ
///
/// [`FontType::ALL`] の順序で `(FontType, &mut T)` を返します。
pub struct FontMapIterMut<'a, T> {
  /// 走査対象
  inner: &'a mut HashMap<FontType, T>,
  /// 現在位置
  index: usize,
}

impl<'a, T> Iterator for FontMapIterMut<'a, T> {
  type Item = (FontType, &'a mut T);

  fn next(&mut self) -> Option<Self::Item> {
    if self.index >= FontType::ALL.len() {
      return None;
    }
    let font_type = FontType::ALL[self.index];
    self.index += 1;
    let value = self.inner.get_mut(&font_type)?;
    // SAFETY: 借用の寿命を `&mut self` から `'a` へ延長する。`index` は単調増加し
    // `FontType::ALL` の要素は一意なので、同じキーを 2 回返すことはなく、
    // 返した `&mut T` どうしがエイリアスすることもない。
    let value = unsafe { &mut *std::ptr::from_mut(value) };
    return Some((font_type, value));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl<T> ExactSizeIterator for FontMapIterMut<'_, T> {}

impl<'a, T> IntoIterator for &'a FontMap<T> {
  type IntoIter = FontMapIter<'a, T>;
  type Item = (FontType, &'a T);

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

impl<'a, T> IntoIterator for &'a mut FontMap<T> {
  type IntoIter = FontMapIterMut<'a, T>;
  type Item = (FontType, &'a mut T);

  fn into_iter(self) -> Self::IntoIter { return self.iter_mut(); }
}

#[cfg(test)]
mod tests {
  use super::FontMap;
  use crate::project::font::FontType;

  #[test]
  fn from_all_and_get_round_trip_by_font_type() {
    // Arrange
    let map = FontMap::from_all(FontType::ALL.iter().map(|ft| format!("{ft}")));

    // Act
    let serif_value = map.get(FontType::Serif);

    // Assert
    assert_eq!(serif_value, "Serif", "FontType::Serif の Debug 表記のはず");
    assert_eq!(map.iter().count(), 19, "FontType は 19 種別のはず");
  }
}
