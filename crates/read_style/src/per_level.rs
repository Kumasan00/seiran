//! 見出しレベルごとの値を保持する `PerLevel<T>` 型。
//!
//! `types::HeadingLevel` をインデックスにした 6 要素の配列です。
//! `Style::headings: PerLevel<HeadingStyle>` のように使い、
//! 見出しレベルを `match` で書き分ける箇所を `style.heading(level)` の一行に集約します。
//!
//! TOML 上では `[heading.part]` / `[heading.chapter]` / … の各テーブルが
//! 対応する要素にデシリアライズされます（カスタム `Deserialize` 実装）。

use std::ops::{Index, IndexMut};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use types::HeadingLevel;

/// 見出しレベル（`HeadingLevel`）をインデックスにした 6 要素の配列。
///
/// TOML 上のキーは `HeadingLevel` の `snake_case` 表現
/// （`part` / `chapter` / `section` / `subsection` / `paragraph` / `subparagraph`）。
#[derive(Debug, Clone)]
pub struct PerLevel<T>([T; HeadingLevel::COUNT]);

impl<T> PerLevel<T> {
  /// `HeadingLevel::ALL` の順序に対応する配列から `PerLevel` を構築する
  #[must_use]
  pub fn new(values: [T; HeadingLevel::COUNT]) -> Self { return PerLevel(values); }

  /// レベルごとに値を生成する関数から `PerLevel` を構築する
  ///
  /// `Default::default` の手書き重複を排除するための主要コンストラクタ。
  pub fn from_fn(mut f: impl FnMut(HeadingLevel) -> T) -> Self { return PerLevel(HeadingLevel::ALL.map(&mut f)); }

  /// 全要素を `HeadingLevel::ALL` の順に走査するイテレータを返す
  pub fn iter(&self) -> std::slice::Iter<'_, T> { return self.0.iter(); }

  /// 全要素を `HeadingLevel::ALL` の順に走査する可変イテレータを返す
  pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> { return self.0.iter_mut(); }
}

impl<T> Index<HeadingLevel> for PerLevel<T> {
  type Output = T;

  fn index(&self, level: HeadingLevel) -> &Self::Output { return &self.0[level as usize]; }
}

impl<T> IndexMut<HeadingLevel> for PerLevel<T> {
  fn index_mut(&mut self, level: HeadingLevel) -> &mut Self::Output { return &mut self.0[level as usize]; }
}

impl<T: Default> Default for PerLevel<T> {
  fn default() -> Self { return PerLevel::from_fn(|_| T::default()); }
}

impl<T> PerLevel<T> {
  /// 各要素にレベル名付きでアクセスするイテレータ。
  ///
  /// バリデーションのパスプレフィックス（例 `heading.section`）を構築する用途で使う。
  pub fn iter_with_level(&self) -> impl Iterator<Item = (HeadingLevel, &T)> {
    return HeadingLevel::ALL.iter().copied().zip(self.0.iter());
  }
}

impl<'a, T> IntoIterator for &'a PerLevel<T> {
  type IntoIter = std::slice::Iter<'a, T>;
  type Item = &'a T;

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

impl<'a, T> IntoIterator for &'a mut PerLevel<T> {
  type IntoIter = std::slice::IterMut<'a, T>;
  type Item = &'a mut T;

  fn into_iter(self) -> Self::IntoIter { return self.iter_mut(); }
}

/// TOML を 6 キーの table としてデシリアライズする。
///
/// `T` が `Default + Deserialize` を満たす場合、未指定キーはデフォルト値で埋める。
impl<'de, T> Deserialize<'de> for PerLevel<T>
where
  T: Deserialize<'de> + Default,
{
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    use std::collections::HashMap;

    let mut map: HashMap<HeadingLevel, T> = HashMap::deserialize(deserializer)?;
    let array = HeadingLevel::ALL.map(|level| map.remove(&level).unwrap_or_default());
    if !map.is_empty() {
      let unknown: Vec<_> = map.keys().map(|level| level.command_name()).collect();
      return Err(D::Error::custom(format!("未知の見出しレベル: {unknown:?}")));
    }
    return Ok(PerLevel(array));
  }
}

/// シリアライズは `HeadingLevel::ALL` 順の 6 キー table として書き出す。
impl<T: Serialize> Serialize for PerLevel<T> {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(HeadingLevel::COUNT))?;
    for (level, value) in HeadingLevel::ALL.iter().zip(self.0.iter()) {
      map.serialize_entry(level.command_name(), value)?;
    }
    return map.end();
  }
}

#[cfg(test)]
mod tests {
  use serde::{Deserialize, Serialize};
  use types::HeadingLevel;

  use super::PerLevel;

  #[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
  struct Leaf {
    #[serde(default)]
    value: i32,
  }

  #[test]
  fn from_fn_indexes_by_heading_level() {
    // Arrange / Act
    let per_level: PerLevel<i32> = PerLevel::from_fn(|level| i32::from(level.depth()));

    // Assert
    assert_eq!(per_level[HeadingLevel::Part], 0);
    assert_eq!(per_level[HeadingLevel::Subparagraph], 5);
  }

  #[test]
  fn deserializes_partial_table_with_defaults() {
    // Arrange: chapter のみ上書き、他はデフォルト
    let toml = "[chapter]\nvalue = 42\n";

    // Act
    let per_level: PerLevel<Leaf> = toml::from_str(toml).expect("partial table should parse");

    // Assert
    assert_eq!(per_level[HeadingLevel::Chapter].value, 42);
    assert_eq!(per_level[HeadingLevel::Part].value, 0);
    assert_eq!(per_level[HeadingLevel::Section].value, 0);
  }

  #[test]
  fn rejects_unknown_level_key() {
    // Arrange: 未知のレベル名
    let toml = "[oops]\nvalue = 1\n";

    // Act
    let result: Result<PerLevel<Leaf>, _> = toml::from_str(toml);

    // Assert
    assert!(result.is_err(), "unknown level should be rejected: {result:?}");
  }

  #[test]
  fn deserializes_all_six_levels() {
    // Arrange: 6 レベルすべてを明示
    let toml = "
[part]
value = 1
[chapter]
value = 2
[section]
value = 3
[subsection]
value = 4
[paragraph]
value = 5
[subparagraph]
value = 6
";

    // Act
    let per_level: PerLevel<Leaf> = toml::from_str(toml).expect("all six levels should parse");

    // Assert
    for (i, level) in HeadingLevel::ALL.iter().enumerate() {
      assert_eq!(per_level[*level].value, i32::try_from(i).unwrap() + 1);
    }
  }
}
