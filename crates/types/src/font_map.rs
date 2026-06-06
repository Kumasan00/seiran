//! 全フォント種別に対応する値を保持する汎用コンテナ [`FontMap`] とそのイテレータ
//!
//! 19 個の個別フィールドを書く代わりに [`FontMap<T>`] を使うことで、
//! [`crate::FontType`] の定義変更だけでフォント種別の追加・削除を完結させられます。

use std::collections::HashMap;

use crate::FontType;

/// 全フォント種別 ([`FontType`]) に対応する値を保持する汎用コンテナ
///
/// 内部的には `HashMap<FontType, T>` を使用しますが、イテレーション時は
/// [`FontType::ALL`] の順序で要素を返します。
///
/// 19 個の個別フィールドを手書きする代わりにこの型を使用することで、
/// フォント種別の追加・削除が `FontType` の定義変更だけで済むようになり、
/// ボイラープレートの `match` 分岐やカスタムイテレータが不要になります。
///
/// # 型パラメータ
///
/// * `T` - 各フォント種別に対応付ける値の型
///
/// # Examples
///
/// ```
/// use types::{FontMap, FontType};
///
/// let map = FontMap::from_all(FontType::ALL.iter().map(|ft| format!("{ft}")));
/// assert_eq!(map.get(FontType::Serif), "Serif");
/// assert_eq!(map.iter().count(), 19);
/// ```
#[derive(Debug, Clone)]
pub struct FontMap<T> {
  inner: HashMap<FontType, T>,
}

impl<T> FontMap<T> {
  /// `FontType::ALL` の順序に対応するイテレータから `FontMap` を構築します
  ///
  /// 渡されたイテレータの各要素を `FontType::ALL[0]`, `FontType::ALL[1]`, ...
  /// と順に対応付けて格納します。
  ///
  /// # Arguments
  ///
  /// * `values` - 19 個の値を順に返すイテレータ
  ///
  /// # Panics
  ///
  /// イテレータの要素数が `FontType::ALL` の要素数（19）と一致しない場合にパニックします。
  pub fn from_all(values: impl IntoIterator<Item = T>) -> Self {
    let inner: HashMap<FontType, T> = FontType::ALL.into_iter().zip(values).collect();
    assert_eq!(inner.len(), FontType::ALL.len(), "FontMap: 要素数が FontType::ALL と一致しません");
    return Self { inner };
  }

  /// 指定されたフォント種別に対応する値への不変参照を返します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別に対応する `&T`
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  /// `from_all` で正しく構築されていれば発生しません。
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &T { return &self.inner[&font_type]; }

  /// 指定されたフォント種別に対応する値への可変参照を返します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別に対応する `&mut T`
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  #[must_use]
  pub fn get_mut(&mut self, font_type: FontType) -> &mut T {
    #[allow(clippy::expect_used)]
    return self.inner.get_mut(&font_type).expect("FontMap: 指定された FontType が見つかりません");
  }

  /// `FontType::ALL` の順序で `(FontType, &T)` のペアを返すイテレータを取得します
  ///
  /// # Returns
  ///
  /// 19 フォント種別を定義順で反復するイテレータ
  #[must_use]
  pub fn iter(&self) -> FontMapIter<'_, T> {
    return FontMapIter {
      inner: &self.inner,
      index: 0,
    };
  }

  /// `FontType::ALL` の順序で `(FontType, &mut T)` のペアを返す可変イテレータを取得します
  ///
  /// # Returns
  ///
  /// 19 フォント種別を定義順で可変反復するイテレータ
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
  inner: &'a HashMap<FontType, T>,
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
  inner: &'a mut HashMap<FontType, T>,
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
    // SAFETY: 各 FontType は一意なので、同じキーに2回アクセスすることはない
    let value = self.inner.get_mut(&font_type)?;
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
