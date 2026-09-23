//! 全フォント種別に対応する値を 1 つずつ持つ表 [`FontMap`]。

use std::{
  fmt::{self, Debug, Formatter},
  ops::Index,
};

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::{
  failures::{self, Failures},
  project::font::FontType,
};

/// フォント種別の数（[`FontMap`] のスロット数）
const SLOTS: usize = FontType::ALL.len();

// `FontType::ALL` が宣言順（＝判別子の昇順）に並んでいることをコンパイル時に確かめる。
// `FontMap` は判別子をそのまま配列の添字に使うので、ここが破れると別の種別の値を返してしまう。
const _: () = {
  let mut index = 0;
  while index < SLOTS {
    assert!(FontType::ALL[index] as usize == index, "FontType::ALL は宣言順に並んでいなければならない");
    index += 1;
  }
};

/// 全フォント種別 ([`FontType`]) に対応する値を 1 つずつ持つ読み取り専用の表
///
/// 値は `[T; 19]` に [`FontType::ALL`] の順で並ぶので、全種別が揃っていることは型が保証する
/// （欠けた表・余った表は構築できない）。構築後に変更する経路は無く、種別での参照
/// （`map[font_type]`）だけを提供する。
///
/// 構築は種別から値を作るクロージャを渡す形だけで、並列版（`par_*`）と失敗しうる版（`try_*`）がある。
/// 失敗しうる版は 1 件目で打ち切らず全種別を試し、失敗を [`FontType::ALL`] 順に全件返す — 並列版でも
/// どの種別が先に完了したかは報告順に漏れない（集約は [`failures::collect_in_input_order`] を通す）。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FontMap<T> {
  /// [`FontType::ALL`] の順に並んだ種別ごとの値
  values: [T; SLOTS],
}

impl<T> FontMap<T> {
  /// 各種別の値を `value_of` で作る（[`FontType::ALL`] の順に 1 回ずつ呼ぶ）。
  pub(crate) fn from_fn(value_of: impl FnMut(FontType) -> T) -> Self {
    return Self {
      values: FontType::ALL.map(value_of),
    };
  }

  /// 各種別の値を `value_of` で作る。1 種別でも失敗すれば表は作らず、失敗を全件返す。
  ///
  /// # Errors
  ///
  /// `value_of` が `Err` を返した種別の失敗を [`FontType::ALL`] 順に集めて返す。
  pub(crate) fn try_from_fn<E>(value_of: impl FnMut(FontType) -> Result<T, E>) -> Result<Self, Failures<E>> {
    return Self::from_complete_results(Vec::from(FontType::ALL.map(value_of)));
  }

  /// [`FontMap::from_fn`] の並列版。
  pub(crate) fn par_from_fn(value_of: impl Fn(FontType) -> T + Sync) -> Self
  where
    T: Send,
  {
    let values = FontType::ALL.par_iter().map(|&font_type| return value_of(font_type)).collect::<Vec<T>>();
    return Self::from_complete(values);
  }

  /// [`FontMap::try_from_fn`] の並列版。
  ///
  /// # Errors
  ///
  /// `value_of` が `Err` を返した種別の失敗を、完了順ではなく [`FontType::ALL`] 順に集めて返す。
  pub(crate) fn par_try_from_fn<E>(value_of: impl Fn(FontType) -> Result<T, E> + Sync) -> Result<Self, Failures<E>>
  where
    T: Send,
    E: Send,
  {
    let results = FontType::ALL.par_iter().map(|&font_type| return value_of(font_type)).collect::<Vec<Result<T, E>>>();
    return Self::from_complete_results(results);
  }

  /// 2 つの表の同じ種別の値どうしを `combine` で合わせた表を作る。
  pub(crate) fn zip_with<U, V>(self, other: FontMap<U>, mut combine: impl FnMut(T, U) -> V) -> FontMap<V> {
    let values = self
      .values
      .into_iter()
      .zip(other.values)
      .map(|(value, other_value)| return combine(value, other_value))
      .collect::<Vec<V>>();
    return FontMap::from_complete(values);
  }

  /// [`FontType::ALL`] 順の結果列を、全件成功なら表へ、1 件でも失敗があれば失敗の全件へまとめる。
  fn from_complete_results<E>(results: Vec<Result<T, E>>) -> Result<Self, Failures<E>> {
    return Ok(Self::from_complete(failures::collect_in_input_order(results)?));
  }

  /// [`FontType::ALL`] を 1 対 1 に写した列から表を作る。
  ///
  /// 呼び出し元はこの module 内だけで、どれも [`FontType::ALL`]（または 19 要素の配列）を写した列を渡す。
  /// `Vec` を経由するのは、rayon の `collect` と [`failures::collect_in_input_order`] が配列へ直接
  /// 集められないため。
  fn from_complete(values: Vec<T>) -> Self {
    let Ok(values) = <[T; SLOTS]>::try_from(values) else {
      unreachable!(
        "呼び出し元は FontType::ALL を 1 対 1 に写した列だけを渡し、collect_in_input_order は全件成功なら同数を返す"
      );
    };
    return Self { values };
  }
}

impl<T> Index<FontType> for FontMap<T> {
  type Output = T;

  // 添字は判別子。module 先頭の const 検査が `FontType::ALL` と判別子の順序の一致を保証する。
  fn index(&self, font_type: FontType) -> &T { return &self.values[font_type as usize]; }
}

/// `{種別: 値, ..}` の形で [`FontType::ALL`] 順に出す。
///
/// 配列の derive `Debug` は位置しか出さず、比較失敗時の出力でどの種別の値か読めなくなる。
impl<T: Debug> Debug for FontMap<T> {
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    return formatter.debug_map().entries(FontType::ALL.iter().zip(&self.values)).finish();
  }
}

#[cfg(test)]
mod tests {
  use super::FontMap;
  use crate::project::font::FontType;

  /// Serif と Math だけ失敗させる（`FontType::ALL` の先頭と中ほど）。
  fn fail_serif_and_math(font_type: FontType) -> Result<(), FontType> {
    if matches!(font_type, FontType::Serif | FontType::Math) {
      return Err(font_type);
    }
    return Ok(());
  }

  #[test]
  fn from_fn_stores_each_value_under_its_font_type() {
    let map = FontMap::from_fn(|font_type| return font_type.as_toml_key());

    for font_type in FontType::ALL {
      assert_eq!(map[font_type], font_type.as_toml_key());
    }
  }

  #[test]
  fn par_from_fn_builds_the_same_map_as_from_fn() {
    let sequential = FontMap::from_fn(|font_type| return font_type.as_toml_key());

    let parallel = FontMap::par_from_fn(|font_type| return font_type.as_toml_key());

    assert_eq!(parallel, sequential);
  }

  #[test]
  fn try_from_fn_reports_every_failure_in_font_type_order() {
    let result = FontMap::try_from_fn(fail_serif_and_math);

    let failures: Vec<FontType> = result.expect_err("2 種別が失敗するはず").into_iter().collect();
    assert_eq!(failures, vec![FontType::Serif, FontType::Math]);
  }

  #[test]
  fn par_try_from_fn_reports_every_failure_in_font_type_order() {
    let result = FontMap::par_try_from_fn(fail_serif_and_math);

    let failures: Vec<FontType> = result.expect_err("2 種別が失敗するはず").into_iter().collect();
    assert_eq!(failures, vec![FontType::Serif, FontType::Math]);
  }

  #[test]
  fn try_from_fn_builds_the_map_when_every_font_type_succeeds() {
    let map = FontMap::try_from_fn(|font_type| return Ok::<_, ()>(font_type)).expect("全種別成功のはず");

    for font_type in FontType::ALL {
      assert_eq!(map[font_type], font_type);
    }
  }

  #[test]
  fn zip_with_combines_values_of_the_same_font_type() {
    let keys = FontMap::from_fn(|font_type| return font_type.as_toml_key());
    let font_types = FontMap::from_fn(|font_type| return font_type);

    let zipped = keys.zip_with(font_types, |key, font_type| return (key, font_type));

    for font_type in FontType::ALL {
      assert_eq!(zipped[font_type], (font_type.as_toml_key(), font_type));
    }
  }

  #[test]
  fn debug_lists_values_keyed_by_font_type_in_declaration_order() {
    let map = FontMap::from_fn(|_| return 0u8);

    let text = format!("{map:?}");

    assert!(text.starts_with("{Serif: 0, SerifBold: 0, "), "種別をキーに宣言順で出るはず: {text}");
  }
}
