//! 全実体（枠 × 候補）に対応する値を 1 つずつ持つ表 [`FaceMap`]。

use std::{
  fmt::{self, Debug, Formatter},
  ops::Index,
};

use rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator};
use strum::VariantArray;

use crate::{
  failures::{self, Failures},
  project::font::{FontFace, FontMap, FontType},
};

/// 全実体（[`FontFace`]）に対応する値を 1 つずつ持つ読み取り専用の表
///
/// 枠ごとの候補の列を [`FontType`] の宣言順に持ち、本文の枠は 1 要素以上・`Math` はちょうど 1 要素という形を
/// 構築時（[`FaceMap::from_slots`]）に検査する — だから [`FontFace::primary`] と [`FontFace::MATH`] は
/// どの表でも引ける。形を保つ変換（`map` / `try_map` 等）は同じ形の表を返す。
///
/// 失敗しうる変換（`try_*`）は 1 件目で打ち切らず全実体を試し、失敗を [`FaceMap::faces`] の順に全件返す — 並列版でも
/// どの実体が先に完了したかは報告順に漏れない。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FaceMap<T> {
  /// [`FontType`] の宣言順に並んだ枠ごとの候補の列（候補は config.toml の宣言順）
  slots: FontMap<Vec<T>>,
}

impl<T> FaceMap<T> {
  /// 枠ごとの候補の列から表を作る。本文の枠に候補が無い、または `Math` の候補がちょうど 1 つでなければ `None`。
  pub(crate) fn from_slots(slots: FontMap<Vec<T>>) -> Option<Self> {
    let well_formed = FontType::VARIANTS.iter().all(|&font_type| {
      let count = slots[font_type].len();
      return match font_type {
        FontType::Math => count == 1,
        FontType::Serif
        | FontType::SerifBold
        | FontType::SerifItalic
        | FontType::SerifBoldItalic
        | FontType::SansSerif
        | FontType::SansSerifBold
        | FontType::SansSerifItalic
        | FontType::SansSerifBoldItalic
        | FontType::Monospace
        | FontType::MonospaceBold
        | FontType::MonospaceItalic
        | FontType::MonospaceBoldItalic
        | FontType::JapaneseSerif
        | FontType::JapaneseSerifBold
        | FontType::JapaneseSansSerif
        | FontType::JapaneseSansSerifBold
        | FontType::JapaneseMonospace
        | FontType::JapaneseMonospaceBold => count >= 1,
      };
    });
    return well_formed.then_some(Self { slots });
  }

  /// 枠 `font_type` の候補の値を候補の順に返す（[`FaceMap::from_slots`] の検査で 1 つ以上）。
  pub(crate) fn candidates(&self, font_type: FontType) -> &[T] { return &self.slots[font_type]; }

  /// 全実体を [`FontType`] の宣言順、枠の中は候補の順に返す。
  pub(crate) fn faces(&self) -> impl Iterator<Item = FontFace> + '_ {
    return FontType::VARIANTS.iter().flat_map(|&font_type| {
      return (0..self.slots[font_type].len()).map(move |candidate| return FontFace::new(font_type, candidate));
    });
  }

  /// 各実体の値を `convert` で写した同じ形の表を作る（[`FaceMap::faces`] の順に 1 回ずつ呼ぶ）。
  pub(crate) fn map<U>(&self, mut convert: impl FnMut(FontFace, &T) -> U) -> FaceMap<U> {
    let values = self.faces().map(|face| return convert(face, &self[face])).collect();
    return self.same_shape(values);
  }

  /// 各実体の値を `convert` で写す。1 実体でも失敗すれば表は作らず、失敗を全件返す。
  ///
  /// # Errors
  ///
  /// `convert` が `Err` を返した実体の失敗を [`FaceMap::faces`] の順に集めて返す。
  pub(crate) fn try_map<U, E>(
    &self,
    mut convert: impl FnMut(FontFace, &T) -> Result<U, E>,
  ) -> Result<FaceMap<U>, Failures<E>> {
    let results = self.faces().map(|face| return convert(face, &self[face])).collect();
    return Ok(self.same_shape(failures::collect_in_input_order(results)?));
  }

  /// [`FaceMap::try_map`] の並列版。
  ///
  /// # Errors
  ///
  /// `convert` が `Err` を返した実体の失敗を、完了順ではなく [`FaceMap::faces`] の順に集めて返す。
  pub(crate) fn par_try_map<U, E>(
    &self,
    convert: impl Fn(FontFace, &T) -> Result<U, E> + Sync + Send,
  ) -> Result<FaceMap<U>, Failures<E>>
  where
    T: Sync,
    U: Send,
    E: Send,
  {
    let faces: Vec<FontFace> = self.faces().collect();
    // `IndexedParallelIterator` の `collect::<Vec<_>>()` は入力順を保つ
    let results = faces.into_par_iter().map(|face| return convert(face, &self[face])).collect::<Vec<Result<U, E>>>();
    return Ok(self.same_shape(failures::collect_in_input_order(results)?));
  }

  /// 各実体の値を消費して並列に変換する。1 実体でも失敗すれば表は作らず、失敗を全件返す。
  ///
  /// # Errors
  ///
  /// `convert` が `Err` を返した実体の失敗を、完了順ではなく [`FaceMap::faces`] の順に集めて返す。
  pub(crate) fn par_try_map_into<U, E>(
    self,
    convert: impl Fn(FontFace, T) -> Result<U, E> + Sync + Send,
  ) -> Result<FaceMap<U>, Failures<E>>
  where
    T: Send,
    U: Send,
    E: Send,
  {
    let faces: Vec<FontFace> = self.faces().collect();
    let shape = self.shape();
    let results = faces
      .into_par_iter()
      .zip(self.into_values().collect::<Vec<T>>())
      .map(|(face, value)| return convert(face, value))
      .collect::<Vec<Result<U, E>>>();
    return Ok(FaceMap::with_shape(&shape, failures::collect_in_input_order(results)?));
  }

  /// 2 つの表の同じ実体の値どうしを `combine` で合わせた表を作る。
  ///
  /// # Panics
  ///
  /// 2 つの表の形（枠ごとの候補の数）が違う場合。呼び元は同じ config から作った表だけを渡す。
  pub(crate) fn zip_with<U, V>(self, other: FaceMap<U>, mut combine: impl FnMut(T, U) -> V) -> FaceMap<V> {
    let shape = self.shape();
    assert!(shape == other.shape(), "同じ config から作った表は枠ごとの候補の数が一致する");
    let values = self.into_values().zip(other.into_values()).map(|(value, other)| return combine(value, other));
    return FaceMap::with_shape(&shape, values.collect());
  }

  /// 枠ごとの候補の数
  fn shape(&self) -> FontMap<usize> { return FontMap::from_fn(|font_type| return self.slots[font_type].len()); }

  /// 全実体の値を [`FaceMap::faces`] の順に消費する。
  fn into_values(self) -> impl Iterator<Item = T> { return self.slots.into_values().flatten(); }

  /// [`FaceMap::faces`] の順に並んだ `values` を、この表と同じ形の表にする。
  fn same_shape<U>(&self, values: Vec<U>) -> FaceMap<U> { return FaceMap::with_shape(&self.shape(), values); }

  /// [`FaceMap::faces`] の順に並んだ `values` を、枠ごとの候補の数 `shape` で区切って表にする。
  ///
  /// 呼び元はこの module 内だけで、どれも検査済みの表と同じ形・同じ順の列を渡すので、形の検査は繰り返さない。
  fn with_shape(shape: &FontMap<usize>, values: Vec<T>) -> Self {
    assert!(
      values.len() == FontType::VARIANTS.iter().map(|&font_type| return shape[font_type]).sum::<usize>(),
      "呼び元は検査済みの表の faces() を 1 対 1 に写した列だけを渡す"
    );
    let mut values = values.into_iter();
    return Self {
      slots: FontMap::from_fn(|font_type| return values.by_ref().take(shape[font_type]).collect()),
    };
  }
}

impl<U, V> FaceMap<(U, V)> {
  /// 組の表を、同じ実体に組の第 1 要素を置いた表と第 2 要素を置いた表に分ける。
  pub(crate) fn unzip(self) -> (FaceMap<U>, FaceMap<V>) {
    let shape = self.shape();
    let (firsts, seconds) = self.into_values().unzip::<U, V, Vec<U>, Vec<V>>();
    return (FaceMap::with_shape(&shape, firsts), FaceMap::with_shape(&shape, seconds));
  }
}

impl<T> Index<FontFace> for FaceMap<T> {
  type Output = T;

  fn index(&self, face: FontFace) -> &T {
    let Some(value) = self.slots[face.font_type()].get(face.candidate()) else {
      unreachable!(
        "FontFace は表の faces() か、構築時の形の検査が引けることを保証する primary / MATH からだけ得る: {face}"
      );
    };
    return value;
  }
}

/// `{実体: 値, ..}` の形で [`FaceMap::faces`] の順に出す（実体は設定パスと同じ `serif[0]` の表記）。
impl<T: Debug> Debug for FaceMap<T> {
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    let mut map = formatter.debug_map();
    for face in self.faces() {
      map.key(&format_args!("{face}")).value(&self[face]);
    }
    return map.finish();
  }
}

#[cfg(test)]
mod tests {
  use strum::VariantArray;

  use super::FaceMap;
  use crate::project::font::{FontFace, FontMap, FontType};

  /// 本文の枠は 1 候補、`serif` だけ `serif_count` 候補の形の表（値は実体そのもの）。
  fn faces_with_serif_count(serif_count: usize) -> FaceMap<FontFace> {
    let slots = FontMap::from_fn(|font_type| {
      let count = if font_type == FontType::Serif {
        serif_count
      } else {
        1
      };
      return (0..count).map(|candidate| return FontFace::new(font_type, candidate)).collect();
    });
    return FaceMap::from_slots(slots).expect("本文は 1 候補以上・math は 1 候補なので形は正しいはず");
  }

  #[test]
  fn from_slots_rejects_an_empty_body_slot_and_a_math_slot_without_exactly_one() {
    let slots_with = |serif: usize, math: usize| {
      return FontMap::from_fn(|font_type| {
        let count = match font_type {
          FontType::Serif => serif,
          FontType::Math => math,
          _ => 1,
        };
        return vec![(); count];
      });
    };

    assert!(FaceMap::from_slots(slots_with(0, 1)).is_none(), "本文の枠が空なら作れないはず");
    assert!(FaceMap::from_slots(slots_with(1, 0)).is_none(), "math が 0 候補なら作れないはず");
    assert!(FaceMap::from_slots(slots_with(1, 2)).is_none(), "math が 2 候補なら作れないはず");
    assert!(FaceMap::from_slots(slots_with(3, 1)).is_some(), "本文の枠は複数候補を持てるはず");
  }

  #[test]
  fn faces_follow_font_type_order_then_candidate_order() {
    let map = faces_with_serif_count(2);

    let faces: Vec<FontFace> = map.faces().take(3).collect();

    assert_eq!(
      faces,
      vec![
        FontFace::new(FontType::Serif, 0),
        FontFace::new(FontType::Serif, 1),
        FontFace::new(FontType::SerifBold, 0)
      ]
    );
    assert_eq!(map.faces().count(), FontType::VARIANTS.len() + 1);
    for face in map.faces() {
      assert_eq!(map[face], face, "実体で引くとその実体の値が返るはず");
    }
  }

  #[test]
  fn try_map_reports_failures_in_face_order() {
    let map = faces_with_serif_count(2);
    let check = |face: FontFace| {
      if face == FontFace::new(FontType::Serif, 1) || face == FontFace::MATH {
        return Err(face);
      }
      return Ok(face);
    };

    let sequential = map.try_map(|face, _| return check(face));
    let parallel = map.par_try_map(|face, _| return check(face));
    let consuming = map.par_try_map_into(|face, _| return check(face));

    let expected = vec![FontFace::new(FontType::Serif, 1), FontFace::MATH];
    for result in [sequential, parallel, consuming] {
      let failures: Vec<FontFace> = result.expect_err("2 実体が失敗するはず").into_iter().collect();
      assert_eq!(failures, expected);
    }
  }

  #[test]
  fn shape_preserving_conversions_keep_each_value_under_its_face() {
    let map = faces_with_serif_count(2);

    let mapped = map.map(|face, value| return (face, *value));
    let converted =
      map.clone().par_try_map_into(|face, value| return Ok::<_, ()>((face, value))).expect("成功するはず");
    let zipped = map.clone().zip_with(map.clone(), |left, right| return (left, right));
    let (firsts, seconds) = zipped.clone().unzip();

    for face in map.faces() {
      assert_eq!(mapped[face], (face, face));
      assert_eq!(converted[face], (face, face));
      assert_eq!(zipped[face], (face, face));
      assert_eq!((firsts[face], seconds[face]), (face, face));
    }
  }

  #[test]
  fn debug_lists_values_by_face_in_face_order() {
    let map = faces_with_serif_count(2).map(|face, _| return face.candidate());

    let text = format!("{map:?}");

    assert!(text.starts_with("{serif[0]: 0, serif[1]: 1, serif_bold[0]: 0, "), "{text}");
    assert!(text.contains("math: 0"), "{text}");
  }
}
